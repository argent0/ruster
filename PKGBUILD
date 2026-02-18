# Maintainer: Your Name <youremail@domain.com>
pkgname=ruster
pkgver=0.1.0
pkgrel=1
pkgdesc="Persistent LLM agent with UNIX socket IPC"
arch=('x86_64')
url="https://github.com/argent0/ruster"
license=('MIT')
depends=('gcc-libs')
makedepends=('git' 'rust' 'cargo')
source=("git+https://github.com/argent0/ruster.git")
sha256sums=('SKIP')

options=(!lto)

pkgver() {
  cd "$srcdir/ruster"
  local _ver=$(grep '^version =' Cargo.toml | head -n 1 | cut -d '"' -f 2)
  echo "${_ver}.r$(git rev-list --count HEAD).$(git rev-parse --short HEAD)"
}

build() {
  cd "$srcdir/ruster"
  cargo build --release --locked
}

package() {
  cd "$srcdir/ruster"
  install -Dm755 "target/release/ruster" "$pkgdir/usr/bin/ruster"
  install -Dm644 "README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"
  # optional: install systemd user unit
  install -Dm644 "ruster.service" "$pkgdir/usr/lib/systemd/user/ruster.service"
}
