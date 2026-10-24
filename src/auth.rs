use crate::{config::AppState, error::AppError};
use actix_web::{
    body::{BoxBody, MessageBody},
    dev::{ServiceRequest, ServiceResponse},
    http::header,
    middleware::Next,
    web,
};

pub async fn auth_middleware(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
    let authorized = match (
        req.app_data::<web::Data<AppState>>(),
        req.headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok()),
    ) {
        (Some(state), Some(h)) => h == &*state.expected_auth,
        _ => false,
    };
    if authorized {
        Ok(next.call(req).await?.map_into_boxed_body())
    } else {
        Ok(req
            .error_response(AppError::Unauthorized)
            .map_into_boxed_body())
    }
}
