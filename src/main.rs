use std::collections::HashMap;
use actix_web::{web, App, HttpServer, Responder, HttpResponse};
use actix_web::web::{Data, Json};
use serde_json::json;
use serde::{Deserialize, Serialize};
use std::env;
use actix_web::error::{ErrorBadRequest, ErrorInternalServerError, ErrorNotFound};
use chrono::Utc;
use hyper::{Body, body, Client, Request};
use hyper_trust_dns::TrustDnsResolver;
use url::Url;
use urlencoding::encode;
use uuid::Uuid;
use tokio::sync::{Mutex};

#[derive(Serialize, Deserialize)]
struct OciProcess {
    id: Uuid,
    // We don't know what shape the data submitted to the SRM server has: it can be anything,
    // so we keep it as a JSON Value, which pretty much matches that.
    #[serde(rename = "POST")]
    call_up_posted_data: Option<serde_json::Value>,
    cxml_request: Option<String>,
    #[serde(rename = "cXMLResponse")]
    cxml_response: Option<String>,
}

struct SrmServerData {
    active_processes: Mutex<HashMap<Uuid, OciProcess>>,
    punchout_server_login_uri: Url,
    punchout_server_confirmation_uri: Url,
    self_base_url: Url,
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

const ORDER_REQUEST_TEMPLATE: &str = r###"<?xml version="1.0" encoding="UTF-8"?>
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
                <Identity>punchout.test</Identity>
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

async fn active_oci_processes(data: Data<SrmServerData>) -> impl Responder {
    Json(json!(*data.active_processes.lock().await))
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

    let mut oci_login_parameters = HashMap::from([
        ("HOOK_URL", format!("{}oci-call-up/{}", data.self_base_url, oci_process_id)),
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
        format!(
            "{}{}",
            oci_login_parameters
                .iter()
                .map(|(key, value)| {
                    format!("{}={}", encode(key), encode(value))
                })
                .fold(String::new(), |accumulator, segment| {
                    format!("{}&{}", accumulator, segment)
                })
                .trim_start_matches(['&']),
            // Note: would be nicer to do `oci_login_parameters.extend()`, but parsing the query
            //       seems really complex here.
            login.query()
                .map(|query| {
                    format!("&{}", query)
                })
                .unwrap_or(String::new())
        ).as_str()
    ));

    HttpResponse::Found()
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

    match process {
        None => Err(
            ErrorNotFound(format!("Could not find process {}", oci_process_id))
        ),

        Some(_) => {
            let process = oci_process_id.clone();

            active_processes
                .entry(oci_process_id)
                .and_modify(|existing| {
                    existing.call_up_posted_data = Some(parsed_body)
                });

            Ok(Json(json!({
                "oci": info.clone(),
                "ociProcessId": process
            })))
        }
    }
}

async fn confirm_oci_payment_with_oci_process_id(
    data: Data<SrmServerData>,
    path: web::Path<Uuid>,
    info: web::Query<ConfirmOciPaymentParameters>,
) -> impl Responder {
    let oci_process_id = path.into_inner();

    let mut active_processes = data.active_processes.lock().await;

    let punchout_server_confirmation_uri = data.punchout_server_confirmation_uri.clone();
    let order_request_token = info.cxml_order_request_token.clone();

    match active_processes.get_mut(&oci_process_id) {
        None => Err(
            ErrorNotFound(format!("Could not find process {}", oci_process_id))
        ),

        // Ugly: we are modifying the collection by reference here...
        Some(process) => {
            // Note: there is no simple way to parse POST parameters from OCI parameters:
            //        * `NEW_ITEM-EXT_PRODUCT_ID` with both dashes and underscores (can't match struct)
            //        * `NEW_ITEM-EXT_PRODUCT_ID[x]` with x starting at 1 (can't match `Vec`)
            //        * Tooling in Rust doesn't parse `[]` as an array (contrary to PHP)
            // we will therefore keep it as a `serde_json::Value`, and work with that.

            match process.call_up_posted_data.clone() {
                None => Err(ErrorBadRequest("Call-up must have happened")),

                Some(call_up_data) => {
                    match (
                        call_up_data.get("NEW_ITEM-EXT_PRODUCT_ID[1]")
                            .map(|first_product_id| {
                                first_product_id.as_str()
                            })
                            .flatten(),
                        call_up_data.get("NEW_ITEM-DESCRIPTION[1]")
                            .map(|first_product_description| {
                                first_product_description.as_str()
                            })
                            .flatten(),
                        call_up_data.as_object()
                            .map(|call_up_data_items| {
                                call_up_data_items
                                    .iter()
                                    .filter(|(key, _)| {
                                        key.starts_with("NEW_ITEM-PRICE")
                                    })
                                    .fold(Some(0.0), |acc, (_, price)| {
                                        price
                                            .as_str()
                                            .map(|item_string_price| {
                                                item_string_price
                                                    .parse::<f64>()
                                                    .ok()
                                            })
                                            .flatten()
                                            .map(|float_string_price| {
                                                acc.map(|accumulator_price| {
                                                    float_string_price + accumulator_price
                                                })
                                            })
                                            .flatten()
                                    })
                            })
                            .flatten(),
                        call_up_data.get("NEW_ITEM-PRICE[1]")
                            .map(|first_product_price| {
                                first_product_price.as_str()
                            })
                            .flatten()
                            .map(|first_product_price| {
                                first_product_price.parse::<f64>()
                            })
                    ) {
                        (
                            Some(first_product_id),
                            Some(first_product_description),
                            Some(total_price),
                            Some(Ok(first_product_price))
                        ) => {
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
                                ("item-supplier-part-id", first_product_id.to_string()),
                                ("item-supplier-auxiliary-id", format!("{}_unused-auxiliary-id", oci_process_id.to_string())),
                                ("item-price", first_product_price.to_string()),
                                ("item-language-code", "en".to_string()),
                                ("item-description", first_product_description.to_string()),
                            ]);

                            // close your eyes: we're interpolating strings directly into XML @_@
                            //                  the only reason this is acceptable is because this is a **MOCK**
                            //                  service, but production systems should use XML builders to
                            //                  prevent XSS/XEE/XXE.
                            let xml_string = replacements
                                .iter()
                                .fold(ORDER_REQUEST_TEMPLATE.to_string(), |xml_string, (key, replacement)| {
                                    xml_string.replace(format!("%{}%", key).as_str(), replacement.as_str())
                                });

                            process.cxml_request = Some(xml_string.clone());

                            let cxml_response = Client::builder()
                                .build(TrustDnsResolver::from_system_conf().into_http_connector())
                                .request(
                                    Request::post(punchout_server_confirmation_uri.to_string())
                                        .header("Content-Type", "text/xml")
                                        .header("Content-Encoding", "utf8")
                                        .body(Body::from(xml_string))
                                        .expect(
                                            r###"
                                            This can only fail if the request builder is misused,
                                            like if we skip configuring the URI of a request.
                                            In theory, this will never fail, but the request
                                            builder is not designed to be type-safe in this
                                            regard, so we still have a potential panic here.
                                            "###
                                        )
                                )
                                .await;

                            match cxml_response {
                                Ok(cxml_response) => {
                                    match body::to_bytes(cxml_response.into_body()).await {
                                        Ok(cxml_response_bytes) => {
                                            match String::from_utf8(cxml_response_bytes.to_vec()) {
                                                Ok(cxml_response_string) => {
                                                    process.cxml_response = Some(cxml_response_string);

                                                    Ok(Json(json!(*active_processes)))
                                                },
                                                Err(source) => Err(ErrorInternalServerError(source))
                                            }
                                        },
                                        Err(source) => Err(ErrorInternalServerError(source))
                                    }
                                },
                                Err(source) => Err(ErrorInternalServerError(source))
                            }
                        }
                        _ => Err(ErrorBadRequest("Provided OCI data is not well formed, and the cXML <OrderRequest/> was **NOT** sent."))
                    }
                }
            }
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let state = Mutex::new(HashMap::new());

    let port = env::var("OCI_SRM_SERVER_MOCK_PORT")
        .expect("OCI_SRM_SERVER_MOCK_PORT must be provided")
        .parse()
        .expect("OCI_SRM_SERVER_MOCK_PORT must be an unsigned integer");

    let data = Data::new(SrmServerData {
        active_processes: state,
        punchout_server_login_uri: Url::parse(
            env::var("PUNCHOUT_SERVER_LOGIN_URI")
                .expect("PUNCHOUT_SERVER_LOGIN_URI must be provided")
                .as_str()
        ).expect("PUNCHOUT_SERVER_LOGIN_URI must be a valid URI"),
        punchout_server_confirmation_uri: Url::parse(
            env::var("PUNCHOUT_SERVER_CONFIRMATION_URI")
                .unwrap()
                .as_str()
        ).expect("PUNCHOUT_SERVER_CONFIRMATION_URI must be a valid URI"),
        self_base_url: Url::parse(
            env::var("OCI_SRM_SERVER_MOCK_BASE_URL")
                .expect("OCI_SRM_SERVER_MOCK_BASE_URL must be provided")
                .as_str()
        ).expect("OCI_SRM_SERVER_MOCK_BASE_URL must be a valid URL")
    });

    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .route("/active-oci-processes", web::get().to(active_oci_processes))
            .route("/start-oci", web::get().to(start_oci))
            .route("/oci-call-up/{ociProcessId}", web::post().to(oci_call_up_with_oci_process_id))
            .route("/confirm-oci-payment/{ociProcessId}", web::get().to(confirm_oci_payment_with_oci_process_id))
    })
        .bind(("0.0.0.0", port))?
        .run()
        .await
}