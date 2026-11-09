//! Blocking HTTP client for the CareLibre REST API.
//!
//! The client is deliberately small and transport-only. Higher-level state
//! (connection health, session lifecycle) lives in [`crate::server`].

use std::time::Duration;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::contact::{
    ApiContactDetail, ApiContactList, ApiContactWrite, ApiRelationship, ApiRelationshipGraph,
    ApiRelationshipHistory, ApiRelationshipList, ApiRelationshipWrite, RelationshipChange,
};
use crate::rostering::{
    ApiAssignment, ApiAssignmentUpdate, ApiAssignmentWrite, ApiBatchShiftWrite, ApiCopyShiftWrite,
    ApiCostCentre, ApiDivision, ApiService, ApiShift, ApiShiftDetail, ApiShiftList, ApiShiftUpdate,
    ApiShiftWrite, ApiSite, ApiSiteWrite, ApiSuitability,
};

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8484";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Clone)]
pub enum ApiError {
    InvalidUrl(String),
    Network(String),
    Http {
        status: u16,
        code: String,
        message: String,
    },
    Decode(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl(message) => write!(f, "{message}"),
            Self::Network(message) => write!(f, "Could not reach the server: {message}"),
            Self::Http { message, .. } => write!(f, "{message}"),
            Self::Decode(message) => write!(f, "Unexpected response from the server: {message}"),
        }
    }
}

impl std::error::Error for ApiError {}

/// Normalise a user-entered server address into a base URL without a trailing slash.
pub fn normalize_base_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Enter the address of your CareLibre server.".to_owned());
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("http://{trimmed}")
    };

    let parsed = url::Url::parse(&candidate)
        .map_err(|err| format!("That doesn't look like a valid URL: {err}"))?;

    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("The server address must start with http:// or https://.".to_owned());
    }
    if parsed.host_str().is_none() {
        return Err(
            "The server address needs a host, for example http://127.0.0.1:8484.".to_owned(),
        );
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("The server address must not contain credentials.".to_owned());
    }

    Ok(candidate.trim_end_matches('/').to_owned())
}

#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub version: String,
    pub database_up: bool,
    pub ready_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserProfile {
    pub id: String,
    pub email: String,
    pub full_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrganisationProfile {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub edition: String,
    pub role: String,
}

/// Token pair plus the profile returned by the authentication endpoints.
///
/// Deliberately does not implement `Debug` so tokens cannot leak into logs.
#[derive(Clone, Deserialize)]
pub struct AuthSession {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub user: UserProfile,
    pub organisation: OrganisationProfile,
}

#[derive(Deserialize)]
struct Liveness {
    version: String,
}

#[derive(Deserialize)]
struct Readiness {
    database: String,
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

#[derive(Serialize)]
struct LoginRequest<'a> {
    email: &'a str,
    password: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    organisation_slug: Option<&'a str>,
}

#[derive(Serialize)]
struct RegisterRequest<'a> {
    organisation_name: &'a str,
    organisation_slug: &'a str,
    full_name: &'a str,
    email: &'a str,
    password: &'a str,
}

#[derive(Serialize)]
struct RefreshRequest<'a> {
    refresh_token: &'a str,
}

#[derive(Serialize)]
struct LogoutRequest<'a> {
    refresh_token: &'a str,
}

pub struct ApiClient {
    base_url: String,
    agent: ureq::Agent,
}

impl ApiClient {
    pub fn new(base_url: &str) -> ApiResult<Self> {
        let base_url = normalize_base_url(base_url).map_err(ApiError::InvalidUrl)?;

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .user_agent(concat!("CareLibre-Desktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();

        Ok(Self { base_url, agent })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Check liveness and database readiness in one operation.
    pub fn health(&self) -> ApiResult<ServerInfo> {
        let health: Liveness = self.get_json("/healthz")?;

        let (database_up, ready_error) = match self.get_json::<Readiness>("/readyz") {
            Ok(readiness) => (readiness.database == "up", None),
            Err(ApiError::Http { message, .. }) => (false, Some(message)),
            Err(err) => (false, Some(err.to_string())),
        };

        Ok(ServerInfo {
            version: health.version,
            database_up,
            ready_error,
        })
    }

    pub fn login(
        &self,
        email: &str,
        password: &str,
        organisation_slug: Option<&str>,
    ) -> ApiResult<AuthSession> {
        self.post_json(
            "/api/v1/auth/login",
            &LoginRequest {
                email,
                password,
                organisation_slug,
            },
        )
    }

    pub fn register(
        &self,
        organisation_name: &str,
        organisation_slug: &str,
        full_name: &str,
        email: &str,
        password: &str,
    ) -> ApiResult<AuthSession> {
        self.post_json(
            "/api/v1/auth/register",
            &RegisterRequest {
                organisation_name,
                organisation_slug,
                full_name,
                email,
                password,
            },
        )
    }

    /// Rotate a refresh token. The old token becomes invalid.
    pub fn refresh(&self, refresh_token: &str) -> ApiResult<AuthSession> {
        self.post_json("/api/v1/auth/refresh", &RefreshRequest { refresh_token })
    }

    pub fn logout(&self, refresh_token: &str) -> ApiResult<()> {
        let (status, body) = self.post("/api/v1/auth/logout", &LogoutRequest { refresh_token })?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(error_from_body(status, &body))
        }
    }

    pub fn list_contacts(
        &self,
        access_token: &str,
        role: &str,
        search: &str,
        after: Option<(&str, &str)>,
        limit: usize,
    ) -> ApiResult<ApiContactList> {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("limit", &limit.to_string());
        query.append_pair("role", role);
        if !search.trim().is_empty() {
            query.append_pair("search", search.trim());
        }
        if let Some((name, id)) = after {
            query.append_pair("after_name", name);
            query.append_pair("after_id", id);
        }
        let path = format!("/api/v1/contacts?{}", query.finish());
        self.get_json_with_token(&path, access_token)
    }

    pub fn get_contact(&self, access_token: &str, id: &str) -> ApiResult<ApiContactDetail> {
        self.get_json_with_token(&format!("/api/v1/contacts/{id}"), access_token)
    }

    pub fn update_contact(
        &self,
        access_token: &str,
        id: &str,
        payload: &ApiContactWrite,
    ) -> ApiResult<ApiContactDetail> {
        let (status, body) = self.send_with_token(
            "PATCH",
            &format!("/api/v1/contacts/{id}"),
            access_token,
            payload,
        )?;
        decode(status, &body)
    }

    pub fn create_contact(
        &self,
        access_token: &str,
        payload: &ApiContactWrite,
    ) -> ApiResult<ApiContactDetail> {
        let (status, body) =
            self.send_with_token("POST", "/api/v1/contacts", access_token, payload)?;
        decode(status, &body)
    }

    /// Relationships touching a contact (or an organisation).
    pub fn list_relationships(
        &self,
        access_token: &str,
        contact_id: Option<&str>,
        organisation_id: Option<&str>,
    ) -> ApiResult<ApiRelationshipList> {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        if let Some(id) = contact_id {
            query.append_pair("contact_id", id);
        }
        if let Some(id) = organisation_id {
            query.append_pair("organisation_id", id);
        }
        query.append_pair("limit", "1000");
        let path = format!("/api/v1/relationships?{}", query.finish());
        self.get_json_with_token(&path, access_token)
    }

    /// The whole-tenant relationship graph, used as the party lookup catalogue.
    pub fn relationship_graph(&self, access_token: &str) -> ApiResult<ApiRelationshipGraph> {
        self.get_json_with_token("/api/v1/relationships/graph", access_token)
    }

    pub fn create_relationship(
        &self,
        access_token: &str,
        payload: &ApiRelationshipWrite,
    ) -> ApiResult<ApiRelationship> {
        let (status, body) =
            self.send_with_token("POST", "/api/v1/relationships", access_token, payload)?;
        decode(status, &body)
    }

    /// Apply one lifecycle change: end, transfer, change role, correct or remove.
    pub fn change_relationship(
        &self,
        access_token: &str,
        id: &str,
        change: &RelationshipChange,
    ) -> ApiResult<()> {
        let blank = |value: &str| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_owned())
        };
        let (method, path, body) = match change {
            RelationshipChange::End {
                end_date,
                reason,
                note,
            } => (
                "POST",
                format!("/api/v1/relationships/{id}/end"),
                serde_json::json!({
                    "end_date": end_date,
                    "reason": reason,
                    "note": blank(note),
                }),
            ),
            RelationshipChange::Transfer {
                effective_date,
                replace,
                new_kind,
                new_id,
                reason,
                note,
            } => (
                "POST",
                format!("/api/v1/relationships/{id}/transfer"),
                serde_json::json!({
                    "effective_date": effective_date,
                    "replace": replace,
                    "new_kind": new_kind,
                    "new_id": new_id,
                    "reason": reason,
                    "note": blank(note),
                }),
            ),
            RelationshipChange::ChangeRole {
                effective_date,
                from_role,
                to_role,
                reason,
                note,
            } => (
                "POST",
                format!("/api/v1/relationships/{id}/change-role"),
                serde_json::json!({
                    "effective_date": effective_date,
                    "from_role": from_role,
                    "to_role": to_role,
                    "reason": reason,
                    "note": blank(note),
                }),
            ),
            RelationshipChange::Correct { relationship, note } => {
                let mut body = serde_json::to_value(relationship)
                    .map_err(|err| ApiError::Decode(err.to_string()))?;
                body["note"] = serde_json::Value::String(note.clone());
                ("PATCH", format!("/api/v1/relationships/{id}"), body)
            }
            RelationshipChange::Remove { reason } => (
                "POST",
                format!("/api/v1/relationships/{id}/remove"),
                serde_json::json!({ "reason": reason }),
            ),
        };
        let (status, body) = self.send_with_token(method, &path, access_token, &body)?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(error_from_body(status, &body))
        }
    }

    /// A relationship's history, including the relationships it replaced.
    pub fn relationship_history(
        &self,
        access_token: &str,
        id: &str,
    ) -> ApiResult<ApiRelationshipHistory> {
        self.get_json_with_token(&format!("/api/v1/relationships/{id}/history"), access_token)
    }

    fn delete_with_token(&self, path: &str, access_token: &str) -> ApiResult<()> {
        let mut response = self
            .agent
            .delete(self.endpoint(path))
            .header("authorization", format!("Bearer {access_token}"))
            .call()
            .map_err(map_transport_error)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|err| ApiError::Network(err.to_string()))?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(error_from_body(status, &body))
        }
    }

    // Rostering -----------------------------------------------------------

    pub fn list_sites(&self, access_token: &str) -> ApiResult<Vec<ApiSite>> {
        self.get_json_with_token("/api/v1/sites", access_token)
    }

    pub fn create_site(&self, access_token: &str, payload: &ApiSiteWrite) -> ApiResult<ApiSite> {
        let (status, body) =
            self.send_with_token("POST", "/api/v1/sites", access_token, payload)?;
        decode(status, &body)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn list_shifts(
        &self,
        access_token: &str,
        from: Option<&str>,
        to: Option<&str>,
        status: Option<&str>,
        site_id: Option<&str>,
        division_id: Option<&str>,
        service_id: Option<&str>,
        cost_centre_id: Option<&str>,
    ) -> ApiResult<ApiShiftList> {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        if let Some(from) = from {
            query.append_pair("from", from);
        }
        if let Some(to) = to {
            query.append_pair("to", to);
        }
        if let Some(status) = status.filter(|status| *status != "all") {
            query.append_pair("status", status);
        }
        if let Some(site_id) = site_id {
            query.append_pair("site_id", site_id);
        }
        if let Some(division_id) = division_id {
            query.append_pair("division_id", division_id);
        }
        if let Some(service_id) = service_id {
            query.append_pair("service_id", service_id);
        }
        if let Some(cost_centre_id) = cost_centre_id {
            query.append_pair("cost_centre_id", cost_centre_id);
        }
        query.append_pair("limit", "1000");
        self.get_json_with_token(&format!("/api/v1/shifts?{}", query.finish()), access_token)
    }

    pub fn list_divisions(&self, access_token: &str) -> ApiResult<Vec<ApiDivision>> {
        self.get_json_with_token("/api/v1/divisions", access_token)
    }

    pub fn list_services(&self, access_token: &str) -> ApiResult<Vec<ApiService>> {
        self.get_json_with_token("/api/v1/services", access_token)
    }

    pub fn list_cost_centres(&self, access_token: &str) -> ApiResult<Vec<ApiCostCentre>> {
        self.get_json_with_token("/api/v1/cost-centres", access_token)
    }

    pub fn create_shifts(
        &self,
        access_token: &str,
        payload: &ApiBatchShiftWrite,
    ) -> ApiResult<ApiShiftList> {
        let (status, body) =
            self.send_with_token("POST", "/api/v1/shifts/batch", access_token, payload)?;
        decode(status, &body)
    }

    pub fn copy_shift(
        &self,
        access_token: &str,
        id: &str,
        payload: &ApiCopyShiftWrite,
    ) -> ApiResult<ApiShiftDetail> {
        let (status, body) = self.send_with_token(
            "POST",
            &format!("/api/v1/shifts/{id}/copy"),
            access_token,
            payload,
        )?;
        decode(status, &body)
    }

    pub fn delete_shift(&self, access_token: &str, id: &str) -> ApiResult<()> {
        self.delete_with_token(&format!("/api/v1/shifts/{id}"), access_token)
    }

    pub fn get_shift(&self, access_token: &str, id: &str) -> ApiResult<ApiShiftDetail> {
        self.get_json_with_token(&format!("/api/v1/shifts/{id}"), access_token)
    }

    pub fn create_shift(&self, access_token: &str, payload: &ApiShiftWrite) -> ApiResult<ApiShift> {
        let (status, body) =
            self.send_with_token("POST", "/api/v1/shifts", access_token, payload)?;
        decode(status, &body)
    }

    pub fn update_shift(
        &self,
        access_token: &str,
        id: &str,
        payload: &ApiShiftUpdate,
    ) -> ApiResult<ApiShift> {
        let (status, body) = self.send_with_token(
            "PATCH",
            &format!("/api/v1/shifts/{id}"),
            access_token,
            payload,
        )?;
        decode(status, &body)
    }

    pub fn create_assignment(
        &self,
        access_token: &str,
        shift_id: &str,
        payload: &ApiAssignmentWrite,
    ) -> ApiResult<ApiAssignment> {
        let (status, body) = self.send_with_token(
            "POST",
            &format!("/api/v1/shifts/{shift_id}/assignments"),
            access_token,
            payload,
        )?;
        decode(status, &body)
    }

    pub fn update_assignment(
        &self,
        access_token: &str,
        id: &str,
        payload: &ApiAssignmentUpdate,
    ) -> ApiResult<ApiAssignment> {
        let (status, body) = self.send_with_token(
            "PATCH",
            &format!("/api/v1/assignments/{id}"),
            access_token,
            payload,
        )?;
        decode(status, &body)
    }

    pub fn delete_assignment(&self, access_token: &str, id: &str) -> ApiResult<()> {
        self.delete_with_token(&format!("/api/v1/assignments/{id}"), access_token)
    }

    pub fn shift_suitability(
        &self,
        access_token: &str,
        shift_id: &str,
    ) -> ApiResult<ApiSuitability> {
        self.get_json_with_token(
            &format!("/api/v1/shifts/{shift_id}/suitability"),
            access_token,
        )
    }

    fn get_json_with_token<T: DeserializeOwned>(
        &self,
        path: &str,
        access_token: &str,
    ) -> ApiResult<T> {
        let mut response = self
            .agent
            .get(self.endpoint(path))
            .header("authorization", format!("Bearer {access_token}"))
            .call()
            .map_err(map_transport_error)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|err| ApiError::Network(err.to_string()))?;
        decode(status, &body)
    }

    fn send_with_token<B: Serialize>(
        &self,
        method: &str,
        path: &str,
        access_token: &str,
        body: &B,
    ) -> ApiResult<(u16, String)> {
        let payload =
            serde_json::to_string(body).map_err(|err| ApiError::Decode(err.to_string()))?;
        let request = match method {
            "POST" => self.agent.post(self.endpoint(path)),
            "PATCH" => self.agent.patch(self.endpoint(path)),
            _ => {
                return Err(ApiError::Network(format!(
                    "unsupported HTTP method {method}"
                )));
            }
        };
        let mut response = request
            .header("authorization", format!("Bearer {access_token}"))
            .header("content-type", "application/json")
            .send(payload)
            .map_err(map_transport_error)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|err| ApiError::Network(err.to_string()))?;
        Ok((status, body))
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn get_json<T: DeserializeOwned>(&self, path: &str) -> ApiResult<T> {
        let mut response = self
            .agent
            .get(self.endpoint(path))
            .call()
            .map_err(map_transport_error)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|err| ApiError::Network(err.to_string()))?;
        decode(status, &body)
    }

    fn post_json<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> ApiResult<T> {
        let (status, body) = self.post(path, body)?;
        decode(status, &body)
    }

    fn post<B: Serialize>(&self, path: &str, body: &B) -> ApiResult<(u16, String)> {
        let payload =
            serde_json::to_string(body).map_err(|err| ApiError::Decode(err.to_string()))?;
        let mut response = self
            .agent
            .post(self.endpoint(path))
            .header("content-type", "application/json")
            .send(payload)
            .map_err(map_transport_error)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|err| ApiError::Network(err.to_string()))?;
        Ok((status, body))
    }
}

fn decode<T: DeserializeOwned>(status: u16, body: &str) -> ApiResult<T> {
    if (200..300).contains(&status) {
        serde_json::from_str(body).map_err(|err| ApiError::Decode(err.to_string()))
    } else {
        Err(error_from_body(status, body))
    }
}

fn error_from_body(status: u16, body: &str) -> ApiError {
    if let Ok(envelope) = serde_json::from_str::<ErrorEnvelope>(body) {
        ApiError::Http {
            status,
            code: envelope.error.code,
            message: envelope.error.message,
        }
    } else {
        ApiError::Http {
            status,
            code: format!("http_{status}"),
            message: format!("The server returned HTTP {status}."),
        }
    }
}

fn map_transport_error(err: ureq::Error) -> ApiError {
    match err {
        ureq::Error::Timeout(_) => {
            ApiError::Network("the server did not respond in time.".to_owned())
        }
        ureq::Error::HostNotFound => ApiError::Network("the host could not be found.".to_owned()),
        other => ApiError::Network(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_urls() {
        assert_eq!(
            normalize_base_url(" 127.0.0.1:8484/ ").unwrap(),
            "http://127.0.0.1:8484"
        );
        assert_eq!(
            normalize_base_url("https://care.example.com/").unwrap(),
            "https://care.example.com"
        );
        assert_eq!(
            normalize_base_url("http://localhost:8484/api").unwrap(),
            "http://localhost:8484/api"
        );
    }

    #[test]
    fn rejects_invalid_urls() {
        assert!(normalize_base_url("").is_err());
        assert!(normalize_base_url("ftp://care.example.com").is_err());
        assert!(normalize_base_url("http://user:pass@care.example.com").is_err());
    }

    #[test]
    fn parses_the_standard_error_envelope() {
        let err = error_from_body(
            401,
            r#"{"error":{"code":"invalid_credentials","message":"Email or password is incorrect"}}"#,
        );

        match err {
            ApiError::Http {
                status,
                code,
                message,
            } => {
                assert_eq!(status, 401);
                assert_eq!(code, "invalid_credentials");
                assert_eq!(message, "Email or password is incorrect");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
