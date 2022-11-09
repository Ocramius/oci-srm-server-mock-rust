use std::collections::HashMap;
use actix_web::{web, App, HttpServer, Responder, HttpResponse};
use actix_web::http::{StatusCode};
use actix_web::web::{Data};
use serde_json::json;
use serde::{Deserialize, Serialize};
use std::env;
use chrono::Utc;
use hyper::{Body, body, Client, Method, Request};
use url::Url;
use urlencoding::encode;
use uuid::Uuid;
use tokio::sync::{Mutex};

#[derive(Serialize, Deserialize)]
struct OciProcess {
    id: Uuid,
    // We don't know what shape the data submitted to the SRM server has: it can be anything,
    // so we keep it as a JSON Value, which pretty much matches that.
    #[serde(alias = "POST")]
    call_up_posted_data: Option<serde_json::Value>,
    cxml_request: Option<String>,
    #[serde(alias = "cXMLResponse")]
    cxml_response: Option<String>,
}

struct SrmServerData {
    active_processes: Mutex<HashMap<Uuid, OciProcess>>,
    punchout_server_login_uri: Url,
    punchout_server_confirmation_uri: Url,
}

#[derive(Deserialize)]
struct StartOciParameters {
    #[serde(alias = "goToProduct")]
    // Naming of this property is pre-existing, do not change it:
    go_to_product: Option<u64>,
}

#[derive(Deserialize)]
struct ConfirmOciPaymentParameters {
    #[serde(alias = "cxmlOrderRequestToken")]
    cxml_order_request_token: String,
}

async fn active_oci_processes(data: Data<SrmServerData>) -> impl Responder {
    let data = data.active_processes.lock().await;

    HttpResponse::Ok()
        .insert_header(("Content-Type", "application/json"))
        .body(json!(*data).to_string())
}

async fn start_oci(
    data: Data<SrmServerData>,
    info: web::Query<StartOciParameters>,
) -> impl Responder {
    let oci_process_id = Uuid::new_v4();

    data.active_processes.lock().await.insert(oci_process_id, OciProcess {
        id: oci_process_id,
        call_up_posted_data: None,
        cxml_request: None,
        cxml_response: None,
    });

    // @TODO start session here? Set OCI id into session.
    //       note: that's only used to verify returning clients.

    let mut oci_login_parameters = HashMap::from([
        ("HOOK_URL", format!("https://oci-srm-server-mock/oci-call-up/{}", oci_process_id)),
        ("OCI_VERSION", "4.0".to_string()),
        ("OPI_VERSION", "1.0".to_string()),
        ("http_content_charset", "utf-8".to_string()),
        ("returntarget", "_parent".to_string()),
    ]);

    let body = match info.go_to_product.clone() {
        Some(n) => HashMap::from([
            ("PRODUCTID", n.to_string()),
            ("FUNCTION", "DETAILADD".to_string()),
        ]),
        _ => HashMap::new()
    };

    oci_login_parameters.extend(body);

    let mut login = data.punchout_server_login_uri.clone();

    login.set_query(Some(
        oci_login_parameters
            .iter()
            .map(|(key, value)| {
                format!("{}={}", encode(key), encode(value))
            })
            .fold(String::new(), |accumulator, segment| {
                format!("{}&{}", accumulator, segment)
            })
            .trim_start_matches(['&'])
    ));

    HttpResponse::Ok()
        .status(StatusCode::FOUND)
        // @TODO add session header?
        .insert_header(("Location", login.to_string()))
        .finish()
}

async fn oci_call_up_with_oci_process_id(
    data: Data<SrmServerData>,
    path: web::Path<Uuid>,
    info: web::Form<serde_json::Value>,
) -> impl Responder {
    let oci_process_id = path.into_inner();

    let mut active_processes = data.active_processes.lock().await;

    let process = active_processes
        .get_mut(&oci_process_id);

    let parsed_body = info.clone();

    // @TODO verify session here? The client should be the same one that created this process.

    match process {
        None => HttpResponse::NotFound()
            .body(format!("Could not find process {}", oci_process_id)),

        Some(_) => {
            let process = oci_process_id.clone();

            active_processes
                .entry(oci_process_id)
                .and_modify(|mut existing| {
                    existing.call_up_posted_data = Some(parsed_body)
                });

            HttpResponse::Ok()
                .insert_header(("Content-Type", "application/json"))
                .body(json!({
                    "oci": info.clone(),
                    "ociProcessId": process
                }).to_string())
        }
    }
}

async fn confirm_oci_payment_with_oci_process_id(
    data: Data<SrmServerData>,
    path: web::Path<Uuid>,
    info: web::Query<ConfirmOciPaymentParameters>,
) -> impl Responder {
    let order_request_template = r###"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE cXML SYSTEM "http://xml.cxml.org/schemas/cXML/1.2.014/cXML.dtd">
<cXML payloadID="%unique-id%" timestamp="%timestamp%">
    <Header>
        <From>
            <Credential domain="SystemID">
                <Identity>nobody cares</Identity>
                <SharedSecret>%cxml-order-request-token%</SharedSecret>
            </Credential>
        </From>
        <To>
            <Credential domain="NetworkId">
                <Identity>punchout.crowdfox.test</Identity>
            </Credential>
        </To>
        <Sender>
            <Credential domain="NetworkId">
                <Identity>customer-system</Identity>
            </Credential>
            <UserAgent>A cXML installation</UserAgent>
        </Sender>
    </Header>
    <Request>
        <OrderRequest>
            <OrderRequestHeader orderID="%order-id%" orderDate="%order-date%">
                <Total>
                    <Money currency="EUR">%order-amount%</Money>
                </Total>
                <ShipTo>
                    <Address>
                        <Name xml:lang="de">%ship-to-final-client-name%</Name>
                        <PostalAddress>
                            <DeliverTo>%ship-to-deliver-to%</DeliverTo>
                            <Street>%ship-to-street%</Street>
                            <City>%ship-to-city%</City>
                            <PostalCode>%ship-to-postal%</PostalCode>
                            <Country isoCountryCode="%ship-to-country-code%">%ship-to-country%</Country>
                        </PostalAddress>
                    </Address>
                </ShipTo>
                <BillTo>
                    <Address>
                        <Name xml:lang="de">%bill-to-final-client-name%</Name>
                        <PostalAddress>
                            <DeliverTo>%bill-to-deliver-to%</DeliverTo>
                            <Street>%bill-to-street%</Street>
                            <City>%bill-to-city%</City>
                            <PostalCode>%bill-to-postal%</PostalCode>
                            <Country isoCountryCode="%bill-to-country-code%">%bill-to-country%</Country>
                        </PostalAddress>
                        <Email>me@example.com</Email>
                    </Address>
                </BillTo>
            </OrderRequestHeader>
            <ItemOut quantity="10">
                <ItemID>
                    <SupplierPartID>%item-supplier-part-id%</SupplierPartID>
                    <SupplierPartAuxiliaryID>%item-supplier-auxiliary-id%</SupplierPartAuxiliaryID>
                </ItemID>
                <ItemDetail>
                    <UnitPrice>
                        <Money currency="EUR">%item-price%</Money>
                    </UnitPrice>
                    <Description xml:lang="%item-language-code%">%item-description%</Description>
                    <UnitOfMeasure>H87</UnitOfMeasure> <!-- H87 = piece -->
                    <Classification domain="SupplierPartID">%item-supplier-part-id%</Classification>
                </ItemDetail>
            </ItemOut>
        </OrderRequest>
    </Request>
</cXML>
    "###;

    let oci_process_id = path.into_inner();

    let mut active_processes = data.active_processes.lock().await;

    let process = active_processes
        .get_mut(&oci_process_id);

    let punchout_server_confirmation_uri = data.punchout_server_confirmation_uri.clone();
    let order_request_token = info.cxml_order_request_token.clone();

    match process {
        None => HttpResponse::NotFound()
            .body(format!("Could not find process {}", oci_process_id)),

        // @TODO Ugly: we are modifying the collection by reference...
        Some(mut process) => {
            process.cxml_request = Some(order_request_template.to_string());
            // @TODO this was more explicit, but created two references to `process`: trouble
            // active_processes
            //     .entry(oci_process_id)
            //     .and_modify(|mut existing| {
            //         existing.cxml_request = Some(order_request_template.to_string())
            //     });

            // Note: there is no simple way to parse POST parameters from OCI parameters:
            //        * `NEW_ITEM-EXT_PRODUCT_ID` with both dashes and underscores (can't match struct)
            //        * `NEW_ITEM-EXT_PRODUCT_ID[x]` with x starting at 1 (can't match `Vec`)
            //        * Tooling in Rust doesn't parse `[]` as an array (contrary to PHP)
            // we will therefore keep it as a `serde_json::Value`, and work with that.

            // @TODO all these execution branches crash: they should be replaced with a parser,
            //       but the input format is just too horrendous.
            //       once the worker crashed, the shared state is also lost (mutex unusable),
            //       so we really need to solve this :-)
            let call_up_data = process.call_up_posted_data
                .clone()
                .expect("Call-up must have happened");

            let first_product_id: String = call_up_data.get("NEW_ITEM-EXT_PRODUCT_ID[1]")
                .expect("NEW_ITEM-EXT_PRODUCT_ID[1] must exist")
                .as_str()
                .expect("NEW_ITEM-EXT_PRODUCT_ID[1] must be a string")
                .to_string(); // @TODO check these conversions with quotes

            let first_product_description: String = call_up_data.get("NEW_ITEM-DESCRIPTION[1]")
                .expect("NEW_ITEM-DESCRIPTION[1] must exist")
                .as_str()
                .expect("NEW_ITEM-DESCRIPTION[1] must be a string")
                .to_string(); // @TODO check these conversions with quotes

            let total_price = call_up_data.as_object()
                .expect("Input data must be a hashmap")
                .iter()
                .filter(|(key, _)| {
                    key.starts_with("NEW_ITEM-PRICE")
                })
                .fold(0.0, |acc, (_, price)| {
                    let float_price: f64 = price.as_str()
                        .expect("Price must be given")
                        .parse()
                        .expect("Price must be a f64");

                    acc + float_price
                });

            let first_product_price: f64 = call_up_data.get("NEW_ITEM-PRICE[1]")
                .expect("NEW_ITEM-PRICE[1] must exist")
                .as_str()
                .expect("Price must be given")
                .parse()
                .expect("Price must be a f64");

            let now = Utc::now()
                .to_rfc3339();
            let now1 = now.clone();
            let now2 = now.clone();

            let replacements = Vec::from([
                ("cxml-order-request-token", order_request_token),
                ("unique-id", Uuid::new_v4().to_string()),
                ("timestamp", now1),
                ("order-id", format!("{}-order-id", oci_process_id.to_string())),
                ("order-date", now2),
                ("order-amount", total_price.to_string()),
                ("ship-to-final-client-name", "Example Company Ltd.".to_string()),
                ("ship-to-deliver-to", "John Doe".to_string()),
                ("ship-to-street", "Short Street 123/B".to_string()),
                ("ship-to-city", "Nowhere".to_string()),
                ("ship-to-postal", "12312".to_string()),
                ("ship-to-country-code", "AT".to_string()),
                ("ship-to-country", "Austria".to_string()),
                ("bill-to-final-client-name", "Example Company Ltd. Billing".to_string()),
                ("bill-to-deliver-to", "Jane Duh".to_string()),
                ("bill-to-street", "Long Street 456/C2".to_string()),
                ("bill-to-city", "Somewhere".to_string()),
                ("bill-to-postal", "23423".to_string()),
                ("bill-to-country-code", "UK".to_string()),
                ("bill-to-country", "United Kingdom".to_string()),
                ("item-supplier-part-id", first_product_id),
                ("item-supplier-auxiliary-id", format!("{}_unused-auxiliary-id", oci_process_id.to_string())),
                ("item-price", first_product_price.to_string()),
                ("item-language-code", "en".to_string()),
                ("item-description", first_product_description),
            ]);

            // close your eyes: we're interpolating strings directly into XML @_@
            //                  the only reason this is acceptable is because this is a **MOCK**
            //                  service, but production systems should use XML builders to
            //                  prevent XSS/XEE/XXE.
            let xml_string = replacements
                .iter()
                .fold(order_request_template.to_string(), |xml_string, (key, replacement)| {
                    xml_string.replace(format!("%{}%", key).as_str(), replacement.as_str())
                });

            let response = Client::new()
                .request(
                    Request::builder()
                        .method(Method::POST)
                        .uri(punchout_server_confirmation_uri.to_string())
                        .header("Content-Type", "text/xml")
                        .header("Content-Encoding", "utf8")
                        .body(Body::from(xml_string))
                        .expect("Request assembled")
                )
                .await
                .expect("Could not read response contents");

            process.cxml_response = Some(
                String::from_utf8(
                    body::to_bytes(response.into_body())
                        .await
                        .expect("Failed to wait for response body to be streamed")
                        .to_vec()
                ).expect("Could not convert response body to a string")
            );

            HttpResponse::Ok()
                .insert_header(("Content-Type", "application/json"))
                .body(json!(*active_processes).to_string())
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let state = Mutex::new(HashMap::new());

    let data = Data::new(SrmServerData {
        active_processes: state,
        // @TODO unwrap is unsafe here: can we improve? Some no_panic could help...
        punchout_server_login_uri: Url::parse(env::var("PUNCHOUT_SERVER_LOGIN_URI").unwrap().as_str())
            .expect("PUNCHOUT_SERVER_LOGIN_URI must be a valid URI"),
        // @TODO unwrap is unsafe here: can we improve? Some no_panic could help...
        punchout_server_confirmation_uri: Url::parse(env::var("PUNCHOUT_SERVER_CONFIRMATION_URI").unwrap().as_str())
            .expect("PUNCHOUT_SERVER_CONFIRMATION_URI must be a valid URI"),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .route("/active-oci-processes", web::get().to(active_oci_processes))
            .route("/start-oci", web::get().to(start_oci))
            .route("/oci-call-up/{ociProcessId}", web::post().to(oci_call_up_with_oci_process_id))
            .route("/confirm-oci-payment/{ociProcessId}", web::get().to(confirm_oci_payment_with_oci_process_id))
    })
        .bind(("0.0.0.0", 8089))?
        .run()
        .await
}