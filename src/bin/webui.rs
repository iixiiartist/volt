#[cfg(feature = "webui")]
fn main() {
    use volt::webui::app::App;
    dioxus::launch(App);
}

#[cfg(not(feature = "webui"))]
fn main() {}
