#[cfg(test)]
mod tests {
    use reqwest::{Client, multipart};
    async fn setup_test_server() -> String {
        "http://localhost:6767".to_string()
    }

    #[tokio::test]
    async fn test_handle_to_bytes() {
        let client: Client = Client::new();
        let base_url = setup_test_server().await;
        let url = format!("{}/api/to-bytes", base_url);
        let form = multipart::Form::new()
            .text("template", "<h1>Test PDF</h1>")
            .text("filename", "my_test.pdf")
            .text("width", "8in")
            .text("height", "10in");
        let response = client
            .post(&url)
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
        let client: Client = Client::new();
        let url = format!("{}/api/to-s3", setup_test_server().await);
        let form = multipart::Form::new()
            .text("template", "<h1>S3 Test</h1>")
            .text("data", r#"{"name": "Gemini"}"#);
        let response = client.post(&url).multipart(form).send().await.unwrap();
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
        let client: Client = Client::new();
        let base_url = setup_test_server().await;
        let url = format!("{}/api/to-s3", base_url);
        let form = multipart::Form::new().text("width", "10in");
        let response = client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .expect("Failed to send request");
        assert_eq!(response.status(), 400);
        let body = response.text().await.unwrap();
        assert_eq!(body, "Missing template");
    }
}
