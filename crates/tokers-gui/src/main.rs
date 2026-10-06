//! Public build: the UI runs, but without a request signer every API call fails
//! with "this build has no request signer". Release builds link a real
//! [`tokers::Backend`] and call [`tokers_gui::run`] with it.

fn main() {
    tokers_gui::run(tokers::Backend::unsigned());
}
