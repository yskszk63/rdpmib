use base64::Engine;
use dbus::MethodErr;
use dbus::blocking::Connection;
use dbus_crossroads::Context;
use dbus_crossroads::Crossroads;
use serde::Deserialize;
use serde::Serialize;
use url::Url;
use thiserror::Error;

struct RunContext<F>
where F: FnMut(String) -> Result<String, String> + Send {
    get_authcode: F,
}

#[derive(Debug, Error)]
pub enum DBusError {
    #[error("{0}")]
    DBus(#[from] dbus::Error),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    environment: Option<String>,
    given_name: Option<String>,
    home_account_id: Option<String>,
    local_account_id: Option<String>,
    username: String,
    name: Option<String>,
    password_expiry: Option<u32>,
    realm: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GetAccountsReply {
    accounts: Vec<Account>,
}

fn get_accounts<F>(
    _: &mut Context,
    _: &mut RunContext<F>,
    (_, _, _): (String, String, String),
) -> Result<(String,), MethodErr>
where F: FnMut(String) -> Result<String, String> + Send {
    let reply = GetAccountsReply {
        accounts: vec![Account {
            environment: None,
            given_name: None,
            home_account_id: None,
            local_account_id: None,
            username: "DUMMY".into(),
            name: None,
            password_expiry: None,
            realm: "00000000-0000-0000-0000-000000000000".into(),
        }],
    };
    let reply = serde_json::to_string(&reply).map_err(|v| MethodErr::failed(&v.to_string()))?;
    Ok((reply,))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PopParams {
    // authentication_scheme: String,
    // uri_host: String,
    // http_method: String,
    kid: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthParameters {
    // account: Account,
    authority: String,
    // authorization_type: u32,
    client_id: String,
    pop_params: Option<PopParams>,
    redirect_uri: String,
    requested_scopes: Vec<String>,
    // username: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcquireTokenSilentryPayload {
    auth_parameters: AuthParameters,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrokerTokenResponse {
    access_token: String,
    access_token_type: String,
    client_info: String,
    expires_on: u32,
    id_token: String,
    granted_scopes: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcquireTokenSilentryReply {
    broker_token_response: BrokerTokenResponse,
}

fn build_auth_url(params: &AuthParameters) -> Result<Url, MethodErr> {
    let url = Url::parse_with_params(
        &format!("{}/oauth2/v2.0/authorize", params.authority),
        &[
            ("client_id", params.client_id.as_str()),
            ("response_type", "code"),
            ("response_mode", "query"),
            ("scope", params.requested_scopes.join(" ").as_str()),
            ("redirect_uri", params.redirect_uri.as_str()),
        ],
    )
    .map_err(|v| MethodErr::invalid_arg(&v.to_string()))?;

    Ok(url)
}

#[derive(Debug, Serialize)]
struct ReqCnf {
    kid: String,
}

#[derive(Debug, Deserialize)]
struct GetTokenResponse {
    // token_type: String,
    // scope: String,
    // expires_in: u32,
    // ext_expires_in: u32,
    access_token: String,
}

fn get_token(params: &AuthParameters, code: &str, kid: &str) -> Result<String, MethodErr> {
    let req_cnf = ReqCnf {
        kid: kid.to_string(),
    };
    let req_cnf = serde_json::to_string(&req_cnf).map_err(|v| MethodErr::failed(&v.to_string()))?;
    let req_cnf = base64::engine::general_purpose::STANDARD.encode(req_cnf);

    let url = Url::parse(&format!("{}/oauth2/v2.0/token", params.authority))
        .map_err(|v| MethodErr::failed(&v.to_string()))?;
    let client = reqwest::blocking::Client::new();
    let res = client
        .post(url)
        .form(&[
            ("client_id", params.client_id.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("scope", params.requested_scopes.join(" ").as_str()),
            ("req_cnf", &req_cnf),
            ("redirect_uri", &params.redirect_uri),
        ])
        .send()
        .map_err(|v| MethodErr::failed(&v.to_string()))?;
    res.error_for_status_ref()
        .map_err(|v| MethodErr::failed(&v.to_string()))?;
    let body = res
        .json::<GetTokenResponse>()
        .map_err(|v| MethodErr::failed(&v.to_string()))?;

    Ok(body.access_token)
}

fn acquire_token_silentry<F>(
    _: &mut Context,
    cx: &mut RunContext<F>,
    (_, _, payload): (String, String, String),
) -> Result<(String,), MethodErr>
where F: FnMut(String) -> Result<String, String> + Send {
    let payload = serde_json::from_str::<AcquireTokenSilentryPayload>(&payload)
        .map_err(|v| MethodErr::failed(&v.to_string()))?;

    let Some(pop) = &payload.auth_parameters.pop_params else {
        return Err(MethodErr::invalid_arg("no pop_params"));
    };

    // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/e967ebeb-9e9f-443e-857a-5208802943c2
    let url = build_auth_url(&payload.auth_parameters)?;
    let code = (cx.get_authcode)(url.to_string()).map_err(|s| MethodErr::failed(&s))?;
    let token = get_token(&payload.auth_parameters, &code, &pop.kid)?;

    let reply = AcquireTokenSilentryReply {
        broker_token_response: BrokerTokenResponse {
            access_token: token,
            access_token_type: "".into(),
            client_info: "".into(),
            expires_on: 0,
            id_token: "".into(),
            granted_scopes: payload.auth_parameters.requested_scopes.join(" "),
        },
    };
    let reply = serde_json::to_string(&reply).map_err(|v| MethodErr::failed(&v.to_string()))?;
    Ok((reply,))
}

pub fn run<F>(get_authcode: F) -> Result<(), DBusError>
where F: FnMut(String) -> Result<String, String> + Send + 'static {
    let conn = Connection::new_session()?;
    conn.request_name("com.microsoft.identity.broker1", false, false, false)?;

    let mut cr = Crossroads::new();

    let token = cr.register::<RunContext<F>, _, _>("com.microsoft.identity.Broker1", |b| {
        b.method("getAccounts", ("a", "b", "c"), ("reply",), get_accounts);
        b.method(
            "acquireTokenSilently",
            ("a", "b", "c"),
            ("reply",),
            acquire_token_silentry,
        );
    });

    let cx = RunContext { get_authcode };
    cr.insert("/com/microsoft/identity/broker1", &[token], cx);

    cr.serve(&conn)?;
    Ok(())
}
