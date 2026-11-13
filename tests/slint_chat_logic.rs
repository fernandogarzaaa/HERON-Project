// Compile and execute the chat lane's pure state machine before it is wired
// into the Slint desktop application. This catches regressions ignored by
// the default crate when new modules are not yet included in the binary.
#[path = "../src/ui/chat.rs"]
mod chat;
