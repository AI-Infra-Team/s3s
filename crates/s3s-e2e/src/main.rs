#![forbid(unsafe_code)]
#![deny(
    clippy::all, //
    clippy::cargo, //
    clippy::pedantic, //
    clippy::self_named_module_files, //
)]
#![warn(
    clippy::dbg_macro, //
)]
#![allow(
    clippy::module_name_repetitions, //
    clippy::missing_errors_doc, // TODO
    clippy::missing_panics_doc, // TODO
    clippy::multiple_crate_versions, // TODO: check later
)]

use s3s_test::tcx::TestContext;
use s3s_test::Result;
use s3s_test::TestFixture;
use s3s_test::TestSuite;

use std::fmt;
use std::ops::Not;
use std::sync::Arc;

use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::primitives::ByteStream;
use tracing::{debug, error, warn};

fn check<T, E>(result: Result<T, SdkError<E>>, allowed_codes: &[&str]) -> Result<Option<T>, SdkError<E>>
where
    E: fmt::Debug + ProvideErrorMetadata,
{
    if let Err(SdkError::ServiceError(ref err)) = result {
        if let Some(code) = err.err().code() {
            if allowed_codes.contains(&code) {
                return Ok(None);
            }
        }
    }
    if let Err(ref err) = result {
        error!(?err);
    }
    match result {
        Ok(val) => Ok(Some(val)),
        Err(err) => Err(err),
    }
}

#[tracing::instrument(skip(s3))]
async fn create_bucket(s3: &aws_sdk_s3::Client, bucket: &str) -> Result {
    s3.create_bucket().bucket(bucket).send().await?;
    Ok(())
}

#[tracing::instrument(skip(s3))]
async fn delete_bucket_loose(s3: &aws_sdk_s3::Client, bucket: &str) -> Result {
    let result = s3.delete_bucket().bucket(bucket).send().await;
    check(result, &["NoSuchBucket"])?;
    Ok(())
}

#[tracing::instrument(skip(s3))]
async fn delete_bucket_strict(s3: &aws_sdk_s3::Client, bucket: &str) -> Result {
    s3.delete_bucket().bucket(bucket).send().await?;
    Ok(())
}

#[tracing::instrument(skip(s3))]
async fn delete_object_loose(s3: &aws_sdk_s3::Client, bucket: &str, key: &str) -> Result {
    let result = s3.delete_object().bucket(bucket).key(key).send().await;
    check(result, &["NoSuchKey", "NoSuchBucket"])?;
    Ok(())
}

#[tracing::instrument(skip(s3))]
async fn delete_object_strict(s3: &aws_sdk_s3::Client, bucket: &str, key: &str) -> Result {
    s3.delete_object().bucket(bucket).key(key).send().await?;
    Ok(())
}

struct E2E {
    s3: aws_sdk_s3::Client,
    sts: aws_sdk_sts::Client,
}

impl TestSuite for E2E {
    async fn setup() -> Result<Self> {
        let sdk_conf = aws_config::from_env().load().await;

        let s3 = aws_sdk_s3::Client::from_conf(
            aws_sdk_s3::config::Builder::from(&sdk_conf)
                .force_path_style(true) // FIXME: remove force_path_style
                .build(),
        );

        let sts = aws_sdk_sts::Client::new(&sdk_conf);

        Ok(Self { s3, sts })
    }
}

struct Basic {
    s3: aws_sdk_s3::Client,
}

impl TestFixture<E2E> for Basic {
    async fn setup(suite: Arc<E2E>) -> Result<Self> {
        Ok(Self { s3: suite.s3.clone() })
    }
}

impl Basic {
    async fn test_list_buckets(self: Arc<Self>) -> Result {
        let s3 = &self.s3;

        let buckets = ["test-list-buckets-1", "test-list-buckets-2"];

        {
            for &bucket in &buckets {
                delete_bucket_loose(s3, bucket).await?;
            }
        }

        {
            for &bucket in &buckets {
                create_bucket(s3, bucket).await?;
            }

            let resp = s3.list_buckets().send().await?;
            let bucket_list: Vec<_> = resp.buckets.as_deref().unwrap().iter().filter_map(|b| b.name()).collect();

            for &bucket in &buckets {
                assert!(bucket_list.contains(&bucket));
                s3.head_bucket().bucket(bucket).send().await?;
            }
        }

        {
            for &bucket in &buckets {
                delete_bucket_strict(s3, bucket).await?;
            }
        }

        Ok(())
    }

    async fn test_list_objects(self: Arc<Self>) -> Result {
        let s3 = &self.s3;

        let bucket = "test-list-objects";
        let keys = ["file-1", "file-2", "file-3"];
        let content = "hello world 你好世界 123456 !@#$%😂^&*()";

        {
            for key in &keys {
                delete_object_loose(s3, bucket, key).await?;
            }
            delete_bucket_loose(s3, bucket).await?;
        }

        {
            create_bucket(s3, bucket).await?;

            for &key in &keys {
                s3.put_object()
                    .bucket(bucket)
                    .key(key)
                    .body(ByteStream::from_static(content.as_bytes()))
                    .send()
                    .await?;
            }

            let resp = s3.list_objects_v2().bucket(bucket).send().await?;
            let object_list: Vec<_> = resp.contents.as_deref().unwrap().iter().filter_map(|o| o.key()).collect();

            for &key in &keys {
                assert!(object_list.contains(&key));
                s3.head_object().bucket(bucket).key(key).send().await?;
            }
        }

        {
            for &key in &keys {
                delete_object_strict(s3, bucket, key).await?;
            }
            delete_bucket_strict(s3, bucket).await?;
        }

        Ok(())
    }

    async fn test_get_object(self: Arc<Self>) -> Result {
        let s3 = &self.s3;

        let bucket = "test-get-object";
        let key = "file-1";
        let content = "hello world 你好世界 123456 !@#$%😂^&*()";

        {
            delete_object_loose(s3, bucket, key).await?;
            delete_bucket_loose(s3, bucket).await?;
        }

        {
            create_bucket(s3, bucket).await?;

            s3.put_object()
                .bucket(bucket)
                .key(key)
                .body(ByteStream::from_static(content.as_bytes()))
                .send()
                .await?;

            let resp = s3.get_object().bucket(bucket).key(key).send().await?;

            let body = resp.body.collect().await?;
            let body = String::from_utf8(body.to_vec())?;
            assert_eq!(body, content);
        }

        {
            delete_object_strict(s3, bucket, key).await?;
            delete_bucket_strict(s3, bucket).await?;
        }

        Ok(())
    }
}

struct Put {
    s3: aws_sdk_s3::Client,
    bucket: String,
    key: String,
}

impl TestFixture<E2E> for Put {
    async fn setup(suite: Arc<E2E>) -> Result<Self> {
        let s3 = &suite.s3;
        let bucket = "test-put";
        let key = "file";

        delete_object_loose(s3, bucket, key).await?;
        delete_bucket_loose(s3, bucket).await?;

        create_bucket(s3, bucket).await?;

        Ok(Self {
            s3: suite.s3.clone(),
            bucket: bucket.to_owned(),
            key: key.to_owned(),
        })
    }

    async fn teardown(self) -> Result {
        let Self { s3, bucket, key } = &self;

        delete_object_loose(s3, bucket, key).await?;
        delete_bucket_loose(s3, bucket).await?;

        Ok(())
    }
}

impl Put {
    async fn test_put_object_tiny(self: Arc<Self>) -> Result {
        let s3 = &self.s3;
        let bucket = self.bucket.as_str();
        let key = self.key.as_str();

        let contents = ["", "1", "22", "333"];

        for content in contents {
            s3.put_object()
                .bucket(bucket)
                .key(key)
                .body(ByteStream::from_static(content.as_bytes()))
                .send()
                .await?;

            let resp = s3.get_object().bucket(bucket).key(key).send().await?;
            let body = resp.body.collect().await?;
            let body = String::from_utf8(body.to_vec())?;
            assert_eq!(body, content);
        }

        Ok(())
    }
}

#[allow(clippy::upper_case_acronyms)]
struct STS {
    sts: aws_sdk_sts::Client,
}

impl TestFixture<E2E> for STS {
    async fn setup(suite: Arc<E2E>) -> Result<Self> {
        Ok(Self { sts: suite.sts.clone() })
    }
}

impl STS {
    async fn test_assume_role(self: Arc<Self>) -> Result<()> {
        let sts = &self.sts;

        let result = sts.assume_role().role_arn("example").role_session_name("test").send().await;

        // FIXME: NotImplemented
        if let Err(SdkError::ServiceError(ref err)) = &result {
            if err.raw().status().as_u16() == 501 {
                warn!(?err, "STS:AssumeRole is not implemented");
                return Ok(());
            }
        }

        let resp = result?;

        let credentials = resp.credentials().unwrap();
        assert!(credentials.access_key_id().is_empty().not(), "Expected non-empty access key ID");
        assert!(credentials.secret_access_key().is_empty().not(), "Expected non-empty secret access key");
        assert!(credentials.session_token().is_empty().not(), "Expected session token in the response");

        debug!(ak=?credentials.access_key_id());
        debug!(sk=?credentials.secret_access_key());
        debug!(st=?credentials.session_token());
        debug!(exp=?credentials.expiration());

        Ok(())
    }
}

fn register(tcx: &mut TestContext) {
    macro_rules! case {
        ($s:ident, $x:ident, $c:ident) => {{
            let mut suite = tcx.suite::<$s>(stringify!($s));
            let mut fixture = suite.fixture::<$x>(stringify!($x));
            fixture.case(stringify!($c), $x::$c);
        }};
    }

    case!(E2E, Basic, test_list_buckets);
    case!(E2E, Basic, test_list_objects);
    case!(E2E, Basic, test_get_object);
    case!(E2E, Put, test_put_object_tiny);
    case!(E2E, STS, test_assume_role);
}

s3s_test::main!(register);