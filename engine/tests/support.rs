//! The support report over the whole PowerLSV cube (ENGINE.md 3).

use engine::db::CardDb;
use engine::lower::support::SupportReport;

#[test]
fn cube_support_report() {
    let db = CardDb::cube();
    let r = SupportReport::from_db(db);
    println!("{}", r.render(40));
    assert_eq!(r.total(), 546);
}
