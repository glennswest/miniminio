use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::xml::XmlWriter;

#[derive(Debug)]
pub enum S3Error {
    NoSuchBucket(String),
    NoSuchKey(String),
    BucketAlreadyExists(String),
    BucketNotEmpty(String),
    InvalidBucketName(String),
    InvalidArgument(String),
    NoSuchUpload(String),
    InvalidPart,
    InvalidPartOrder,
    EntityTooLarge,
    AccessDenied,
    SignatureDoesNotMatch,
    MissingSecurityHeader,
    MethodNotAllowed,
    InternalError(String),
}

impl S3Error {
    fn code(&self) -> &str {
        match self {
            Self::NoSuchBucket(_) => "NoSuchBucket",
            Self::NoSuchKey(_) => "NoSuchKey",
            Self::BucketAlreadyExists(_) => "BucketAlreadyExists",
            Self::BucketNotEmpty(_) => "BucketNotEmpty",
            Self::InvalidBucketName(_) => "InvalidBucketName",
            Self::InvalidArgument(_) => "InvalidArgument",
            Self::NoSuchUpload(_) => "NoSuchUpload",
            Self::InvalidPart => "InvalidPart",
            Self::InvalidPartOrder => "InvalidPartOrder",
            Self::EntityTooLarge => "EntityTooLarge",
            Self::AccessDenied => "AccessDenied",
            Self::SignatureDoesNotMatch => "SignatureDoesNotMatch",
            Self::MissingSecurityHeader => "MissingSecurityHeader",
            Self::MethodNotAllowed => "MethodNotAllowed",
            Self::InternalError(_) => "InternalError",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::NoSuchBucket(b) => format!("The specified bucket does not exist: {b}"),
            Self::NoSuchKey(k) => format!("The specified key does not exist: {k}"),
            Self::BucketAlreadyExists(b) => {
                format!("The requested bucket name already exists: {b}")
            }
            Self::BucketNotEmpty(b) => {
                format!("The bucket you tried to delete is not empty: {b}")
            }
            Self::InvalidBucketName(b) => format!("The specified bucket is not valid: {b}"),
            Self::InvalidArgument(m) => m.clone(),
            Self::NoSuchUpload(id) => format!("No such upload: {id}"),
            Self::InvalidPart => "One or more of the specified parts could not be found".into(),
            Self::InvalidPartOrder => "The list of parts was not in ascending order".into(),
            Self::EntityTooLarge => "Your proposed upload exceeds the maximum allowed size".into(),
            Self::AccessDenied => "Access Denied".into(),
            Self::SignatureDoesNotMatch => {
                "The request signature we calculated does not match the signature you provided"
                    .into()
            }
            Self::MissingSecurityHeader => "Missing required security header".into(),
            Self::MethodNotAllowed => "The specified method is not allowed".into(),
            Self::InternalError(m) => format!("Internal server error: {m}"),
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::NoSuchBucket(_) | Self::NoSuchKey(_) | Self::NoSuchUpload(_) => {
                StatusCode::NOT_FOUND
            }
            Self::BucketAlreadyExists(_) => StatusCode::CONFLICT,
            Self::BucketNotEmpty(_) => StatusCode::CONFLICT,
            Self::InvalidBucketName(_) | Self::InvalidArgument(_) => StatusCode::BAD_REQUEST,
            Self::InvalidPart | Self::InvalidPartOrder => StatusCode::BAD_REQUEST,
            Self::EntityTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::AccessDenied | Self::SignatureDoesNotMatch | Self::MissingSecurityHeader => {
                StatusCode::FORBIDDEN
            }
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn to_xml(&self) -> String {
        let mut w = XmlWriter::new();
        w.declaration()
            .open("Error")
            .elem("Code", self.code())
            .elem("Message", &self.message())
            .close("Error");
        w.finish()
    }
}

impl IntoResponse for S3Error {
    fn into_response(self) -> Response {
        let xml = self.to_xml();
        (
            self.status_code(),
            [
                ("content-type", "application/xml"),
                ("x-amz-request-id", "miniminio"),
            ],
            xml,
        )
            .into_response()
    }
}

impl From<std::io::Error> for S3Error {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => Self::NoSuchKey("unknown".into()),
            std::io::ErrorKind::PermissionDenied => Self::AccessDenied,
            _ => Self::InternalError(e.to_string()),
        }
    }
}

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for S3Error {}
