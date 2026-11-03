//! The terminal `devpage` runs in, as the preview's reporter: each problem
//! found while serving is printed on standard error the first time.

use devpage_core::check::Report;
use devpage_core::problem::Problem;

/// The preview's reporter: each problem it is told of printed on standard
/// error.
pub struct Terminal;

impl Report for Terminal {
    fn problem(&self, problem: &Problem) {
        eprintln!("{problem}");
    }
}
