#[cfg(test)]
mod tests {
    use reqwest::{Client, multipart};
    use std::env;

    // Helper to setup URL and grab the token from your .env
    async fn setup_test_env() -> (String, String) {
        // Load the .env file so the test can read the token
        dotenvy::dotenv().ok();

        let base_url = "http://localhost:6767".to_string();
        let token = env::var("API_BEARER_TOKEN").expect("API_BEARER_TOKEN must be set for tests");

        (base_url, token)
    }

    #[tokio::test]
    async fn test_handle_to_bytes() {
        let (base_url, token) = setup_test_env().await;
        let client = Client::new();
        let url = format!("{}/api/to-bytes", base_url);

        let form = multipart::Form::new()
            .text("template", "<h1>Test PDF</h1>")
            .text("filename", "my_test.pdf")
            .text("width", "8in")
            .text("height", "10in");

        let response = client
            .post(&url)
            .bearer_auth(&token) // <-- Inject the Bearer token here
            .multipart(form)
            .send()
            .await
            .expect("Failed to send request to /api/to-bytes");

        assert_eq!(
            response.status(),
            200,
            "Server returned error: {:?}",
            response.text().await
        );

        assert_eq!(response.headers()["content-type"], "application/pdf");

        let content_disposition = response.headers()["content-disposition"]
            .to_str()
            .expect("Missing content-disposition header");
        assert!(content_disposition.contains("my_test.pdf"));

        let bytes = response
            .bytes()
            .await
            .expect("Failed to get response bytes");

        assert!(!bytes.is_empty(), "PDF buffer is empty");
        assert!(bytes.starts_with(b"%PDF"), "Response is not a valid PDF");
    }

    #[tokio::test]
    async fn test_handle_to_s3() {
        let (base_url, token) = setup_test_env().await;
        let client = Client::new();
        let url = format!("{}/api/to-s3", base_url);

        let form = multipart::Form::new()
            .text("template", "<h1>S3 Test</h1>")
            .text("data", r#"{"name": "Gemini"}"#);

        let response = client
            .post(&url)
            .bearer_auth(&token) // <-- Inject the Bearer token here
            .multipart(form)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        let json: serde_json::Value = response.json().await.expect("Failed to parse JSON");
        assert_eq!(json["status"], "success");
        assert!(json["url"].as_str().is_some(), "URL should be present");
        assert!(
            json["storage_key"].as_str().unwrap().starts_with("pdfs/"),
            "Storage key should start with pdfs/"
        );
        assert!(json["file_size"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_missing_template_error() {
        let (base_url, token) = setup_test_env().await;
        let client = Client::new();
        let url = format!("{}/api/to-s3", base_url);

        let form = multipart::Form::new().text("width", "10in");

        let response = client
            .post(&url)
            .bearer_auth(&token) // <-- Inject the Bearer token here
            .multipart(form)
            .send()
            .await
            .expect("Failed to send request");

        assert_eq!(response.status(), 400);

        // Update assertion to match our new JSON error structure
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"], "Missing required field: template");
    }
}
