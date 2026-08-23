#!/usr/bin/env bash
#
# Build, test, and lint the whole Erno monorepo.
#
#   ./build.sh                build every part
#   ./build.sh api cli        build only those parts
#   ./build.sh test           run the Rust test suites
#   ./build.sh help           list every target
#
# There is no cargo workspace: api/, cli/ and monitoring/ are independent
# crates, and app/, admin/, monitoring/ui and docs/ are npm projects. This
# script is the one entry point across all of them.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

step() { printf '\n\033[1;34m==>\033[0m \033[1m%s\033[0m\n' "$1"; }

# node_modules is gitignored, so install on first use rather than failing.
ensure_node_modules() {
    if [ ! -d "$1/node_modules" ]; then
        step "Installing $1 dependencies"
        (cd "$1" && npm install)
    fi
}

build_api() {
    step "Building api (erno crate)"
    (cd api && cargo build --all-features)
}

build_cli() {
    step "Building cli (erno binary)"
    (cd cli && cargo build)
}

build_app() {
    ensure_node_modules app
    step "Building app (erno-angular library)"
    # Explicit project name: angular.json has no application project or
    # defaultProject, so a bare `ng build` has nothing to select.
    (cd app && npm run build -- erno-angular)
}

build_monitoring() {
    step "Building monitoring (collector)"
    (cd monitoring && cargo build)
    ensure_node_modules monitoring/ui
    step "Building monitoring console"
    (cd monitoring/ui && npm run build)
}

build_admin() {
    ensure_node_modules admin
    step "Building admin (operator console)"
    (cd admin && npm run build)
}

build_docs() {
    ensure_node_modules docs
    step "Building docs (Astro site)"
    (cd docs && npm run build)
}

run_test() {
    # api tests need PostgreSQL at postgres://erno:erno@localhost/erno
    # (api/config/test.toml). cli has no tests yet.
    step "Testing api"
    (cd api && cargo test --all-features)
    step "Testing cli"
    (cd cli && cargo test)
    # Uses its own database (erno_monitoring_test); see monitoring/config/test.toml.
    step "Testing monitoring"
    (cd monitoring && cargo test)
    ensure_node_modules app
    step "Testing app (erno-angular)"
    (cd app && npm test -- --watch=false)
}

run_check() {
    step "Checking formatting"
    (cd api && cargo fmt --check)
    (cd cli && cargo fmt --check)
    (cd monitoring && cargo fmt --check)
    step "Running clippy"
    (cd api && cargo clippy --all-features -- -D warnings)
    (cd cli && cargo clippy -- -D warnings)
    (cd monitoring && cargo clippy -- -D warnings)
}

run_fmt() {
    step "Formatting Rust sources"
    (cd api && cargo fmt)
    (cd cli && cargo fmt)
    (cd monitoring && cargo fmt)
}

run_clean() {
    step "Cleaning build outputs"
    (cd api && cargo clean)
    (cd cli && cargo clean)
    (cd monitoring && cargo clean)
    rm -rf app/dist docs/dist admin/dist monitoring/ui/dist
}

usage() {
    cat <<'EOF'
Usage: ./build.sh [target...]

Build targets (all run when no target is given):
  api         Build the erno crate            (cd api  && cargo build --all-features)
  cli         Build the erno binary           (cd cli  && cargo build)
  app         Build the erno-angular library  (cd app  && npm run build -- erno-angular)
  admin       Build the operator console      (cd admin && npm run build)
  monitoring  Build the collector + console   (cd monitoring && cargo build)
  docs        Build the Astro docs site       (cd docs && npm run build)

Other targets:
  test     Run the test suites (Rust suites require PostgreSQL)
  check    cargo fmt --check + clippy -D warnings, all Rust crates
  fmt      cargo fmt, all Rust crates
  clean    cargo clean every crate, remove all dist directories
  help     Show this message

Targets compose, in the order given:  ./build.sh fmt api test
EOF
}

if [ $# -eq 0 ]; then
    build_api
    build_cli
    build_app
    build_admin
    build_monitoring
    build_docs
    step "Done"
    exit 0
fi

for target in "$@"; do
    case "$target" in
        api) build_api ;;
        cli) build_cli ;;
        app) build_app ;;
        admin) build_admin ;;
        monitoring) build_monitoring ;;
        docs) build_docs ;;
        all) build_api; build_cli; build_app; build_admin; build_monitoring; build_docs ;;
        test) run_test ;;
        check) run_check ;;
        fmt) run_fmt ;;
        clean) run_clean ;;
        help | -h | --help) usage; exit 0 ;;
        *)
            echo "Unknown target '$target'." >&2
            echo >&2
            usage >&2
            exit 1
            ;;
    esac
done

step "Done"
