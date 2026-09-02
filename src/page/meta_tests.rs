use super::*;

fn meta_page(root: PageId) -> Page {
    let mut page = Page::new();
    page.init_meta(root);
    page
}

#[test]
fn init_meta_gives_a_readable_meta_page() {
    let page = meta_page(PageId(1));

    assert!(page.is_meta());
    assert_eq!(page.root_page_id(), PageId(1));
    assert_eq!(page.read_u64(OFF_FREE_LIST), 0);
}

#[test]
fn a_zeroed_page_is_not_a_meta_page() {
    // opening someone else's file, or a page that was never initialised
    let page = Page::new();

    assert!(!page.is_meta());
}

#[test]
fn a_wrong_magic_or_version_is_rejected() {
    let mut page = meta_page(PageId(1));
    page.write_u32(OFF_MAGIC, 0xDEAD_BEEF);
    assert!(!page.is_meta());

    let mut page = meta_page(PageId(1));
    page.write_u32(OFF_VERSION, META_VERSION + 1);
    assert!(!page.is_meta());
}

#[test]
fn setting_the_root_leaves_the_rest_alone() {
    // this runs on every root split, so it must not disturb the header
    let mut page = meta_page(PageId(1));

    page.set_root_page_id(PageId(4096));

    assert_eq!(page.root_page_id(), PageId(4096));
    assert!(page.is_meta());
    assert_eq!(page.read_u64(OFF_FREE_LIST), 0);
}

#[test]
fn a_root_id_past_255_survives() {
    // a one byte write would silently truncate this
    let page = meta_page(PageId(300));

    assert_eq!(page.root_page_id(), PageId(300));
}
