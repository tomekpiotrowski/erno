use axum_test::TestResponse;
use lets_expect::{AssertionError, AssertionResult};

fn assert_status_code(response: &TestResponse, expected: u16) -> AssertionResult {
    if response.status_code() == expected {
        Ok(())
    } else {
        Err(AssertionError::new(vec![format!(
            "Expected status code {}, got {}",
            expected,
            response.status_code()
        )]))
    }
}

pub fn respond_ok(response: &TestResponse) -> AssertionResult {
    assert_status_code(response, 200)
}

pub fn respond_created(response: &TestResponse) -> AssertionResult {
    assert_status_code(response, 201)
}

pub fn respond_bad_request(response: &TestResponse) -> AssertionResult {
    assert_status_code(response, 400)
}

pub fn respond_not_found(response: &TestResponse) -> AssertionResult {
    assert_status_code(response, 404)
}
pub fn be_unauthorized(response: &TestResponse) -> AssertionResult {
    assert_status_code(response, 401)
}
