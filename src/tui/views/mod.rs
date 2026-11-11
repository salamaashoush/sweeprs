pub mod confirm;
pub mod deleting;
pub mod detail_panel;
pub mod explore_panel;
pub mod help_bar;
pub mod status_bar;
pub mod tree_panel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Main,
    Confirm,
    Deleting,
    Search,
    Explore,
}
