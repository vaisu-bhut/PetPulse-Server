use reqwest::Client;
use serde_json::{json, Value};
use std::env;
use std::path::Path;
use tokio::fs::File;
use tokio_util::codec::{BytesCodec, FramedRead};

pub struct GeminiClient {
    client: Client,
    api_key: String,
    model: String,
}

impl GeminiClient {
    pub fn new() -> Self {
        let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
        let model = env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-1.5-pro".to_string()); // Default fallback if not set
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }

    pub async fn analyze_video(&self, file_path: &str) -> Result<Value, String> {
        // 1. Upload File
        let file_uri = self.upload_file(file_path).await?;
        
        // 2. Wait for processing (Video processing takes time)
        // Gemini File API requires waiting for state=ACTIVE
        self.wait_for_file_active(&file_uri).await?;

        // 3. Generate Content
        self.generate_content(&file_uri).await
    }

    async fn upload_file(&self, file_path: &str) -> Result<String, String> {
        let path = Path::new(file_path);
        let file_name = path.file_name().unwrap().to_str().unwrap();
        let file = File::open(path).await.map_err(|e| e.to_string())?;
        
        let stream = FramedRead::new(file, BytesCodec::new());
        let file_body = reqwest::Body::wrap_stream(stream);

        // Upload endpoint (Multipart)
        // https://generativelanguage.googleapis.com/upload/v1beta/files
        let url = format!("https://generativelanguage.googleapis.com/upload/v1beta/files?key={}", self.api_key);
        
        // We need to send metadata as well ideally, but simple upload works too?
        // Let's use the Resumable upload or Simple upload. Simple multipart is easier.
        // The API expects 'file' part.
        
        let form = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::stream(file_body).file_name(file_name.to_string()));

        let res = self.client.post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Upload Request Failed: {}", e))?;

        if !res.status().is_success() {
             let text = res.text().await.unwrap_or_default();
             return Err(format!("Upload Failed: {}", text));
        }

        let json: Value = res.json().await.map_err(|e| e.to_string())?;
        let uri = json["file"]["uri"].as_str().ok_or("No URI in response")?.to_string();
        let name = json["file"]["name"].as_str().ok_or("No Name in response")?.to_string();
        
        // We actually need the 'name' (files/...) to check status, and 'uri' to use in generation.
        // Let's return details or just struct.
        // For simplicity, I'll store 'name' in a separate call or just return both?
        // Actually, the 'uri' is used in the prompt, but 'name' is used for GetFile to check state.
        
        Ok(name) // Return the resource name e.g. "files/enc..."
    }

    async fn wait_for_file_active(&self, file_name: &str) -> Result<(), String> {
        let url = format!("https://generativelanguage.googleapis.com/v1beta/{}?key={}", file_name, self.api_key);
        
        // Poll loop
        let mut retries = 0;
        while retries < 60 { // Wait up to 5-10 mins? Videos take time.
             let res = self.client.get(&url).send().await.map_err(|e| e.to_string())?;
             let json: Value = res.json().await.map_err(|e| e.to_string())?;
             
             let state = json["state"].as_str().unwrap_or("UNKNOWN");
             
             if state == "ACTIVE" {
                 return Ok(());
             } else if state == "FAILED" {
                 return Err("Video processing failed by Google".to_string());
             }
             
             tokio::time::sleep(std::time::Duration::from_secs(5)).await;
             retries += 1;
        }
        
        Err("Timeout waiting for video processing".to_string())
    }

    async fn generate_content(&self, file_name: &str) -> Result<Value, String> {
        // Construct the model URL
        // User asked for "Gemini 3.0 Pro". 
        // Note: As of now, only 1.5 is standard, but I'll plug in the env var `GEMINI_MODEL`.
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}", 
            self.model, self.api_key
        );

        let prompt = "Analyze this video of a pet. precise behavior analysis, activity level, and mood. Return a JSON object (without markdown code blocks) with the following structure: \n\
        { \n\
            'summary': 'brief summary string', \n\
            'activities': ['activity1', 'activity2'], \n\
            'mood': 'mood string', \n\
            'is_unusual': boolean, \n\
            'unusual_details': 'details if is_unusual is true, else null' \n\
        } \n\
        Identify if there is any unusual or concerning behavior (e.g., limping, aggression, extreme lethargy) and set 'is_unusual' to true.";

        let body = json!({
            "contents": [{
                "parts": [
                    { "text": prompt },
                    { "file_data": { 
                        "mime_type": "video/mp4", 
                        "file_uri": self.get_uri_from_name(file_name).await? // Wait, we need the URI, not the name?
                        // Actually, the GetFile response contains the URI. 
                        // I should have cached it or fetched it again.
                        // Let's fetch it again in get_uri...
                    }}
                ]
            }]
        });

        let res = self.client.post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Generate Request Failed: {}", e))?;

        if !res.status().is_success() {
             let text = res.text().await.unwrap_or_default();
             return Err(format!("Generate Failed: {}", text));
        }

        let json: Value = res.json().await.map_err(|e| e.to_string())?;
        Ok(json)
    }
    
    // Helper to get URI because upload returns it but I returned name for checking status.
    async fn get_uri_from_name(&self, file_name: &str) -> Result<String, String> {
        let url = format!("https://generativelanguage.googleapis.com/v1beta/{}?key={}", file_name, self.api_key);
        let res = self.client.get(&url).send().await.map_err(|e| e.to_string())?;
        let json: Value = res.json().await.map_err(|e| e.to_string())?;
        json["uri"].as_str().map(|s| s.to_string()).ok_or("URI not found in file info".to_string())
    }
}
