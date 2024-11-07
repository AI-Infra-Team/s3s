use super::ops::Operations;
use super::rust::codegen_doc;

use crate::declare_codegen;

use codegen_writer::g;
use codegen_writer::glines;
use heck::ToSnakeCase;

pub fn codegen(ops: &Operations) {
    declare_codegen!();

    glines![
        "use crate::dto::*;"
        "use crate::error::S3Result;"
        "use crate::request::S3Request;"
        "use crate::response::S3Response;"
        ""
        "/// An async trait which represents the S3 API"
        "#[async_trait::async_trait]"
        "pub trait S3: Send + Sync + 'static {"
        ""
    ];

    for op in ops.values() {
        let method_name = op.name.to_snake_case();
        let input = &op.input;
        let output = &op.output;

        codegen_doc(op.doc.as_deref());
        g!("async fn {method_name}(&self, _req: S3Request<{input}>) -> S3Result<S3Response<{output}>> {{");
        g!("Err(s3_error!(NotImplemented, \"{} is not implemented yet\"))", op.name);
        g!("}}");
        g!();
    }

    g!("}}");
    g!();
}