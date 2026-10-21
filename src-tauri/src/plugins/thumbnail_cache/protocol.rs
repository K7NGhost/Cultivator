use super::*;
use tauri::http::{HeaderValue, Response, StatusCode};

fn parse_token(token: &str) -> Option<(&str, &str)> {
    let (case, image) = token.strip_prefix('/')?.split_once('.')?;
    if [case, image]
        .iter()
        .all(|part| part.len() == 64 && part.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        Some((case, image))
    } else {
        None
    }
}

pub(crate) async fn serve(path: &str) -> Response<Vec<u8>> {
    let result = async {
        let (case_key, image_key) = parse_token(path).ok_or(StatusCode::BAD_REQUEST)?;
        let case = registry()
            .lock()
            .await
            .cases
            .values()
            .find(|case| case.key == case_key)
            .cloned()
            .ok_or(StatusCode::NOT_FOUND)?;
        case.store
            .read(image_key)
            .await
            .map_err(|error| {
                eprintln!("Thumbnail cache read failed: {error}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .ok_or(StatusCode::NOT_FOUND)
    }
    .await;
    let (status, bytes) = match result {
        Ok(bytes) => (StatusCode::OK, bytes),
        Err(status) => (status, Vec::new()),
    };
    let mut response = Response::new(bytes);
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert("Access-Control-Allow-Origin", HeaderValue::from_static("*"));
    headers.insert("Content-Type", HeaderValue::from_static("image/jpeg"));
    headers.insert(
        "Cache-Control",
        HeaderValue::from_static(if status == StatusCode::OK {
            "private, max-age=31536000, immutable"
        } else {
            "no-store"
        }),
    );
    response
}
