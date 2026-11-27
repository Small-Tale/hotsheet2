use super::request_succeeded;

fn response(status: u16) -> ureq::http::Response<&'static str> {
    ureq::http::Response::builder()
        .status(status)
        .body("not a trusted JSON body")
        .unwrap()
}

#[test]
fn hs2_081pxn_presence_success_is_a_nonredirected_response_not_merely_ureq_ok() {
    for status in [200, 201, 204, 299, 300, 301, 302, 303, 304, 307, 308, 399] {
        assert_eq!(request_succeeded(Ok(response(status))), status < 300);
    }
    for status in [400, 401, 404, 503] {
        assert!(!request_succeeded::<&str>(Err(ureq::Error::StatusCode(
            status
        ))));
    }
}
