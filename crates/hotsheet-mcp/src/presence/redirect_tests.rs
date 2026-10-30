use super::request_succeeded;

#[test]
fn hs2_081pxn_presence_success_is_a_nonredirected_response_not_merely_ureq_ok() {
    for status in [200, 201, 204, 299, 300, 301, 302, 303, 304, 307, 308, 399] {
        let response = ureq::Response::new(status, "Synthetic", "not a trusted JSON body").unwrap();
        assert_eq!(request_succeeded(Ok(response)), status < 300);
    }
    for status in [400, 401, 404, 503] {
        let response =
            ureq::Response::new(status, "Synthetic", "private synthetic detail").unwrap();
        assert!(!request_succeeded(Err(ureq::Error::Status(
            status, response
        ))));
    }
}
