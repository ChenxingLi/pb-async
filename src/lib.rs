#![warn(missing_docs)]
#![doc(html_root_url = "https://docs.rs/pb-async/0.1.0/")]
//! Asynchronous PushBullet client for Rust.
//!
//! # Usage
//!
//! To use `pb_async`, first create a [`Client`] with an access token from
//! [the PushBullet account settings].
//!
//! Then you can use any of the request methods on [`Client`] to perform API
//! requests.
//!
//! All requests are futures, so you'll need to run them in some kind of
//! futures execution context. I recommend using the [`tokio`] crate.
//!
//! # Example
//!
//! ```no_run
//! extern crate futures;
//! extern crate pb_async;
//! extern crate tokio;
//!
//! use futures::Future;
//!
//! # fn main() {
//! let client = pb_async::Client::new("...").unwrap();
//!
//! tokio::run(
//!     client.push(
//!         pb_async::PushTarget::SelfUser {},
//!         pb_async::PushData::Note {
//!             title: "",
//!             body: "Hello, user!",
//!         },
//!     ).or_else(|error| {
//!         eprintln!("error: {}", error);
//!         Ok(())
//!     })
//! );
//! # }
//! ```
//!
//! See [`Client`] for more snippets.
//!
//! Or find [full example programs] in the GitHub repository.
//!
//! [`tokio`]: https://crates.io/crates/tokio
//! [full example programs]: https://github.com/daboross/pb-async/tree/master/examples/
//! [the PushBullet account settings]: https://www.pushbullet.com/#settings/account
extern crate bytes;
extern crate failure;
extern crate futures;
extern crate http;
extern crate hyper;
extern crate hyper_tls;
extern crate mpart_async;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate log;
#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate serde_derive;

mod errors;

pub use errors::{RequestError, StartupError};

use http::header::HeaderValue;

static API_ROOT: &str = "https://api.pushbullet.com/v2/";
static TOKEN_HEADER: &str = "Access-Token";

type HyperClient = hyper::Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>;

/// PushBullet client
pub struct Client {
    token: HeaderValue,
    client: HyperClient,
}

impl Client {
    /// Create a new client with a given token.
    ///
    /// Example usage:
    ///
    /// ```no_run
    /// let token = std::env::var("PB_TOKEN").expect("expected PB_TOKEN env var to exist");
    /// let client = pb_async::Client::new(&token)
    ///     .expect("expected client creation to succeed");
    /// ```
    pub fn new(token: &str) -> Result<Self, StartupError> {
        let mut connector = hyper_tls::HttpsConnector::new();
        connector.https_only(true);
        Ok(Client {
            token: HeaderValue::from_str(token)
                .map_err(|e| StartupError::InvalidToken(e, token.to_owned()))?,
            client: hyper::Client::builder()
                .pool_max_idle_per_host(std::usize::MAX)
                .build(connector),
        })
    }

    /// Create a new client with a given token and an existing hyper client.
    pub fn with_client(token: &str, client: HyperClient) -> Result<Self, StartupError> {
        Ok(Client {
            token: HeaderValue::from_str(token)
                .map_err(|e| StartupError::InvalidToken(e, token.to_owned()))?,
            client: client,
        })
    }

    /// Retrieves information of the logged in user.
    ///
    /// Example usage:
    ///
    /// ```no_run
    /// extern crate futures;
    /// extern crate pb_async;
    /// extern crate tokio;
    ///
    /// use futures::Future;
    ///
    /// # fn main() {
    /// # let client = pb_async::Client::new("...").unwrap();
    ///
    /// tokio::executor::spawn(client.get_user().and_then(|user_info| {
    ///     println!("User email is {}", user_info.email);
    ///     Ok(())
    /// }).or_else(|error| {
    ///     eprintln!("error: {}", error);
    ///     Ok(())
    /// }));
    /// # }
    /// ```
    pub async fn get_user(&self) -> Result<User, RequestError> {
        self.get("users/me").await.and_then(|(bytes, data)| {
            serde_json::from_value(data).map_err(|error| RequestError::Json { error, bytes })
        })
    }

    /// Retrieves a list of devices.
    ///
    /// Example usage:
    ///
    /// ```no_run
    /// extern crate futures;
    /// extern crate pb_async;
    /// extern crate tokio;
    ///
    /// use futures::Future;
    ///
    /// # fn main() {
    /// # let client = pb_async::Client::new("...").unwrap();
    ///
    /// tokio::executor::spawn(client.list_devices().and_then(|devices| {
    ///     println!("Devices: {:#?}", devices);
    ///     Ok(())
    /// }).or_else(|error| {
    ///     eprintln!("error: {}", error);
    ///     Ok(())
    /// }));
    /// # }
    /// ```
    pub async fn list_devices(&self) -> Result<Vec<Device>, RequestError> {
        #[derive(Deserialize)]
        struct Devices {
            devices: Vec<Device>,
        }
        self.get("devices").await.and_then(|(bytes, data)| {
            let d: Devices = serde_json::from_value(data).map_err(|error| RequestError::Json {
                error,
                bytes: bytes.clone(),
            })?;
            Ok(d.devices)
        })
    }

    /// Pushes some data to a target.
    ///
    /// Example usage:
    ///
    /// ```no_run
    /// extern crate futures;
    /// extern crate pb_async;
    /// extern crate tokio;
    ///
    /// use futures::Future;
    ///
    /// # fn main() {
    /// # let client = pb_async::Client::new("...").unwrap();
    ///
    /// tokio::executor::spawn(
    ///     client.push(
    ///         pb_async::PushTarget::SelfUser {},
    ///         pb_async::PushData::Note {
    ///             title: "",
    ///             body: "Hello, user!",
    ///         },
    ///     ).or_else(|error| {
    ///         eprintln!("error: {}", error);
    ///         Ok(())
    ///     })
    /// );
    /// # }
    /// ```
    pub async fn push(&self, target: PushTarget, data: PushData) -> Result<(), RequestError> {
        #[derive(Serialize)]
        struct Push {
            #[serde(flatten)]
            data: PushData,
            #[serde(flatten)]
            target: PushTarget,
        }

        let post_data = serde_json::to_string(&Push { target, data }).unwrap();

        self.post("pushes", post_data.into()).await.map(|_resp| ())
    }

    /// Prepares a file for upload prior to pushing it via [`Client::push`].
    ///
    /// This method handles file streaming correctly. If you use a streaming
    /// [`hyper::Body`], it will be correctly wrapped and the resulting
    /// connection won't need to keep the entire file in memory.
    ///
    /// Example usage:
    ///
    /// ```no_run
    /// extern crate futures;
    /// extern crate pb_async;
    /// extern crate tokio;
    ///
    /// use futures::Future;
    ///
    /// # fn main() {
    /// # let client = pb_async::Client::new("...").unwrap();
    /// tokio::executor::spawn(
    ///     client
    ///         .upload_request("hello.txt", "text/plain", "Hello, world!\n".into())
    ///         .and_then(move |file_data| {
    ///             client.push(
    ///                 pb_async::PushTarget::SelfUser {},
    ///                 pb_async::PushData::File {
    ///                     body: "",
    ///                     file_name: &file_data.file_name,
    ///                     file_type: &file_data.file_type,
    ///                     file_url: &file_data.file_url,
    ///                 },
    ///             )
    ///         })
    ///         .or_else(|error| {
    ///             eprintln!("error pushing file: {}", error);
    ///             Ok(())
    ///         }),
    /// );
    /// # }
    /// ```
    pub async fn upload_request(
        &self,
        file_name: &str,
        file_type: &str,
        upload_data: hyper::Body,
    ) -> Result<UploadRequestResponse, RequestError> {
        #[derive(Serialize)]
        struct Upload<'a> {
            file_name: &'a str,
            file_type: &'a str,
        }
        let post_data = serde_json::to_string(&Upload {
            file_name,
            file_type,
        })
        .unwrap();
        let token_for_later_use = self.token.clone();
        let client_for_later_use = self.client.clone();
        let (bytes, data) = self.post("upload-request", post_data.into()).await?;

        let (request, last_response) = {
            use http::header::*;
            let RawUploadRequestResponse {
                file_name,
                file_type,
                file_url,
                upload_url,
            } = serde_json::from_value(data)
                .map_err(|error| RequestError::Json { error, bytes })?;

            let mut mpart = mpart_async::client::MultipartRequest::default();

            mpart.add_stream("file", &*file_name, &*file_type, upload_data);

            let request = hyper::Request::post(upload_url)
                .header(TOKEN_HEADER, token_for_later_use)
                .header(
                    CONTENT_TYPE,
                    &*format!("multipart/form-data; boundary={}", mpart.get_boundary()),
                )
                .body(hyper::Body::wrap_stream(mpart))?;

            (
                request,
                UploadRequestResponse {
                    file_name,
                    file_type,
                    file_url,
                    _priv: (),
                },
            )
        };

        // let (parts, body) = client_for_later_use
        //     .request(request)
        //     .await
        //     .and_then(|response| {
        //         let (parts, body) = response.into_parts();
        //         body.concat2().map(|body| (parts, body))
        //     })
        //     .map_err(Into::into)?;

        let response = client_for_later_use.request(request).await?;

        let (parts, body) = response.into_parts();

        let bytes = hyper::body::to_bytes(body).await?;
        if !parts.status.is_success() {
            return Err(RequestError::Status {
                status: parts.status,
                bytes: bytes,
            });
        }
        Ok(last_response)
    }

    async fn get(
        &self,
        target: &'static str,
    ) -> Result<(bytes::Bytes, serde_json::Value), RequestError> {
        self.request(target, hyper::Body::empty(), http::Method::GET, |b| b)
            .await
    }

    async fn post(
        &self,
        target: &'static str,
        body: hyper::Body,
    ) -> Result<(bytes::Bytes, serde_json::Value), RequestError> {
        let length = hyper::body::HttpBody::size_hint(&body)
            .exact()
            .expect("expected unconditional content length");
        self.request(target, body, http::Method::POST, move |b| {
            b.header(http::header::CONTENT_TYPE, "application/json")
                .header(http::header::CONTENT_LENGTH, &*format!("{}", length))
        })
        .await
    }

    async fn request(
        &self,
        target: &'static str,
        body: hyper::Body,
        method: http::Method,
        extra: impl FnOnce(http::request::Builder) -> http::request::Builder,
    ) -> Result<(bytes::Bytes, serde_json::Value), RequestError> {
        let request = extra(
            hyper::Request::builder()
                .method(method)
                .uri(format!("{}{}", API_ROOT, target))
                .header(TOKEN_HEADER, self.token.clone()),
        )
        .body(body)
        .expect("expected request to be well-formed");
        debug!("sending request: {:?}", request);
        let response = self.client.request(request).await?;

        let (parts, body) = response.into_parts();

        let bytes = hyper::body::to_bytes(body).await?;
        let data: serde_json::Value =
            serde_json::from_slice(&*bytes).map_err(|error| RequestError::Json {
                error,
                bytes: bytes.clone(),
            })?;
        debug!("received json: {:?} from {}", data, target);
        if let Some(err_data) = data.as_object().and_then(|obj| obj.get("error")) {
            #[derive(Deserialize)]
            struct ErrorData {
                code: String,
                message: String,
            }
            if let Ok(ErrorData { code, message }) = serde::Deserialize::deserialize(err_data) {
                return Err(RequestError::Server { code, message });
            }
        }
        if !parts.status.is_success() {
            return Err(RequestError::Status {
                status: parts.status,
                bytes: bytes,
            });
        }
        Ok((bytes, data))
    }
}

/// Target which data can be pushed to.
///
/// Used in [Client::push].
#[derive(Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum PushTarget {
    /// Push to generic self-user stream.
    SelfUser {},
    /// Send to a specific device.
    Device {
        /// Device identifier - see [Device.iden] and [Client::list_devices].
        #[serde(rename = "device_iden")]
        iden: String,
    },
    /// Send to a user by email address, or send by email if this is not a
    /// PushBullet user.
    User {
        /// User email - see [User.email] and [Client::get_user].
        email: String,
    },
    /// Send to all subscribers in a channel by tag.
    Channel {
        /// Channel tag. No way to retrieve this in current crate API.
        #[serde(rename = "channel_tag")]
        tag: String,
    },
    /// Send to all users who have granted access to an OAuth by iden.
    Client {
        /// OAuth client iden. No way to retrieve this in current crate API.
        #[serde(rename = "client_iden")]
        iden: String,
    },
}

/// Data which can be pushed.
///
/// Used in [Client::push].
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum PushData {
    /// Note push.
    Note {
        /// The note's title.
        title: String,
        /// The note's message.
        body: String,
    },
    /// Link push.
    Link {
        /// The link's title.
        title: String,
        /// A message associated with the link.
        body: String,
        /// The url to open.
        url: String,
    },
    /// File push. Needs to be uploaded first with [Client::upload_request].
    File {
        /// A message to go with the file.
        body: String,
        /// The name of the file.
        file_name: String,
        /// The MIME type of the file.
        file_type: String,
        /// The url for the file. See [UploadRequestResponse.file_url].
        file_url: String,
    },
}

/// Information about logged in user.
#[derive(Clone, Debug, Deserialize)]
pub struct User {
    /// Created timestamp in unix time.
    pub created: f64,
    /// Account email - used as a push target
    pub email: String,
    /// Normalized account email
    pub email_normalized: String,
    /// Identifier
    pub iden: String,
    /// URL of profile image
    pub image_url: Option<String>,
    /// Maximum upload size allowed
    pub max_upload_size: f64,
    /// Modified timestamp in unix time.
    pub modified: f64,
    /// User real name
    pub name: String,
    #[serde(default)]
    _priv: (),
}

/// PushBullet device
#[derive(Clone, Debug, Deserialize)]
pub struct Device {
    /// Whether or not this device is active.
    ///
    /// Deleted devices show up as non-active.
    pub active: bool,
    /// Creation timestamp in unix time.
    pub created: f64,
    /// Device identifier - used for sending pushes.
    pub iden: String,
    /// Modified timestamp in unix time.
    pub modified: f64,
    /// Nickname of device
    pub nickname: Option<String>,
    #[serde(default)]
    _priv: (),
}

/// (raw) response to [`Client::upload_request`].
///
/// This is separate since it has the 'upload_url' field we consume.
#[derive(Clone, Debug, Deserialize)]
struct RawUploadRequestResponse {
    file_name: String,
    file_type: String,
    file_url: String,
    upload_url: String,
}

/// Response to [`Client::upload_request`].
#[derive(Clone, Debug)]
pub struct UploadRequestResponse {
    /// The file name that will be used for the file. (may be truncated from
    /// original file name)
    pub file_name: String,
    /// The file type that will be used for the file (may be different
    /// from the one provided to upload_request)
    pub file_type: String,
    /// The URL where the file will be available after it is uploaded.
    pub file_url: String,
    _priv: (),
}
