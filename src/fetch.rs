use futures::executor::block_on;
// use reqwest::blocking::Response;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Request, RequestInit, RequestMode, Response};

pub async fn fetch_url_internal(url: String) -> Result<JsValue, JsValue> {
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);

    let request = Request::new_with_str_and_init(&url, &opts)?;
    let window = web_sys::window().unwrap();
    let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
    let resp: Response = resp_value.dyn_into()?;

    JsFuture::from(resp.text()?).await
}

pub fn xml_http_request(url: String) -> Result<web_sys::XmlHttpRequest, JsValue> {
    let xhr = web_sys::XmlHttpRequest::new()?;
    xhr.open("GET", &url)?;
    xhr.send()?;

    // let response = xhr.response_text()?;
    Ok(xhr)
}

/* pub fn threaded_http<T: Send + Sync>(
    url: String,
    callback: impl FnOnce(Result<reqwest::blocking::Response, reqwest::Error>) -> T + Send + Sync,
) -> Option<T> {
    std::thread::scope(|s| {
        s.spawn(|| {
            let client = reqwest::blocking::Client::builder().build().unwrap();
            callback(client.get(&url).send())
        })
        .join()
        .ok()
    })
} */
