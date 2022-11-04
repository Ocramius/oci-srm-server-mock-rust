use std::collections::HashMap;
use actix_web::{web, App, HttpServer, Responder, HttpResponse};
use actix_web::http::{StatusCode};
use actix_web::web::{Data};
use serde_json::json;
use serde::{Deserialize, Serialize};
use serde;
use std::env;
use std::sync::{Mutex};
use url::Url;
use urlencoding::encode;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct OciProcess {
    // @TODO we probably want to implement Serialize, Deserialize for Uuid,
    //       and fix this once and for all
    id: String,
    // We don't know what shape the data submitted to the SRM server has: it can be anything,
    // so we keep it as a JSON Value, which pretty much matches that.
    #[serde(alias = "POST")]
    call_up_posted_data: Option<serde_json::Value>,
    cxml_request: Option<String>,
    #[serde(alias = "cXMLResponse")]
    cxml_response: Option<String>
}

struct SrmServerData {
    active_processes: Mutex<HashMap<String, OciProcess>>,
    punchout_server_login_uri: Url,
    punchout_server_confirmation_uri: Url,
}

#[derive(Deserialize)]
struct StartOciParameters {
    #[serde(alias = "goToProduct")]
    // Naming of this property is pre-existing, do not change it:
    go_to_product: Option<u64>,
}

async fn active_oci_processes(data: Data<SrmServerData>) -> impl Responder {
    let data = data.active_processes.lock().unwrap();

    HttpResponse::Ok()
        .insert_header(("Content-Type", "application/json"))
        .body(json!(*data).to_string())
}

async fn start_oci(
    data: Data<SrmServerData>,
    info: web::Query<StartOciParameters>,
) -> impl Responder {
    let oci_process_id = Uuid::new_v4();

    data.active_processes.lock().unwrap().insert(oci_process_id.to_string(), OciProcess {
        id: oci_process_id.to_string(),
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

    // @TODO None is not working for cases with malformed go_to_product! We get a 400 error there!
    let body = match info.go_to_product {
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
    path: web::Path<String>,
    info: web::Form<serde_json::Value>,
) -> impl Responder {
    let oci_process_id = path.into_inner().to_string();

    let mut active_processes = data.active_processes.lock().unwrap();

    let process = active_processes
        .get_mut(oci_process_id.as_str());

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
    path: web::Path<String>,
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

    let oci_process_id = path.into_inner().to_string();

    let mut active_processes = data.active_processes.lock().unwrap();

    let process = active_processes
        .get_mut(oci_process_id.as_str());

    let punchout_server_confirmation_uri = data.punchout_server_confirmation_uri.clone();

    match process {
        None => HttpResponse::NotFound()
            .body(format!("Could not find process {}", oci_process_id)),

        Some(process) => {
            active_processes
                .entry(oci_process_id)
                .and_modify(|mut existing| {
                    existing.cxml_request = Some(order_request_template.to_string())
                });

            // @TODO this is blocking: can we do it non-blocking?
            let client = reqwest::Client::new();

            // Note: there is no simple way to parse POST parameters from OCI parameters:
            //        * `NEW_ITEM-EXT_PRODUCT_ID` with both dashes and underscores (can't match struct)
            //        * `NEW_ITEM-EXT_PRODUCT_ID[x]` with x starting at 1 (can't match `Vec`)
            //        * Tooling in Rust doesn't parse `[]` as an array (contrary to PHP)
            // we will therefore keep it as a `serde_json::Value`, and work with that.

            // close your eyes: we're interpolating strings directly into XML @_@
            //                  the only reason this is acceptable is because this is a **MOCK** service,
            //                  but production systems should use XML builders to prevent XSS/XEE/XXE.

            // let call_up_data = process.call_up_posted_data
            //     // @TODO test these crashes, turn them into better errors. Use no_panic!
            //     .expect("Call-up must have happened");
            //
            // let description: String = call_up_data.get("NEW_ITEM-EXT_PRODUCT_ID[1]")
            //     .expect("NEW_ITEM-EXT_PRODUCT_ID[1]")
            //     .into(); // @TODO check these conversions with quotes
            // let prices = call_up_data.as_object()
            //     .iter()
            //     .filter(|key| {
            //         // @TODO check if key starts
            //     })

            // @TODO assemble template here

            let response = client.post(punchout_server_confirmation_uri)
                .body(order_request_template.to_string())
                .header("Content-Type", "text/xml")
                .header("Content-Encoding", "utf8")
                .send()
                .await
                .expect("request performed");

            HttpResponse::Ok()
                .insert_header(("Content-Type", "application/json"))
                .body(json!(*active_processes).to_string())
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let state: Mutex<HashMap<String, OciProcess>> = Mutex::new(HashMap::new());

    state.lock().unwrap().insert("aaa".to_owned(), OciProcess {
        id: Uuid::new_v4().to_string(),
        call_up_posted_data: None,
        cxml_request: None,
        cxml_response: None
    });

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
        .workers(2) // more than enough
        .bind(("127.0.0.1", 8089))?
        .run()
        .await
}