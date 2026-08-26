use xtask::accept;
use xtask::project_root;
use xtask::pushd;
use xtask::Result;

fn main() -> Result<()> {
    let _d = pushd(project_root());

    match xtask::task_command().fallback_to_usage().run() {
        xtask::TaskCommand::Accept => accept::accept(),
    }
}
