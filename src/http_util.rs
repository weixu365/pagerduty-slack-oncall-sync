use lambda_http::{Body, Response};

pub fn response(status_code: u16, body: String) -> Response<Body> {
    Response::builder()
        .status(status_code)
        .body(Body::from(body))
        .unwrap()
}
