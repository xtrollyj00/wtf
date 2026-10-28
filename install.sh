#!/bin/sh
# Installs the latest wtf release for this machine:
#   curl -fsSL https://raw.githubusercontent.com/xtrollyj00/wtf/main/install.sh | sh
# into ~/.local/bin, or $WTF_INSTALL_DIR. Linux and macOS, x86_64 and arm64.
set -eu

case "$(uname -s)" in
    Linux) os=unknown-linux-gnu ;;
    Darwin) os=apple-darwin ;;
    *) echo "wtf: no release for $(uname -s); build it with cargo (see the README)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) echo "wtf: no release for $(uname -m); build it with cargo (see the README)" >&2; exit 1 ;;
esac

name="wtf-$arch-$os"
url="https://github.com/xtrollyj00/wtf/releases/latest/download/$name.tar.gz"
dir="${WTF_INSTALL_DIR:-$HOME/.local/bin}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "downloading $url"
curl -fsSL "$url" -o "$tmp/$name.tar.gz"
curl -fsSL "$url.sha256" -o "$tmp/$name.tar.gz.sha256"
# the archive must match the checksum published with it
if command -v sha256sum >/dev/null; then
    (cd "$tmp" && sha256sum -c "$name.tar.gz.sha256" >/dev/null)
else
    (cd "$tmp" && shasum -a 256 -c "$name.tar.gz.sha256" >/dev/null)
fi
tar xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$dir"
cp "$tmp/$name/wtf" "$dir/wtf"
chmod 755 "$dir/wtf"
echo "installed $("$dir/wtf" --version) to $dir/wtf"
case ":$PATH:" in
    *":$dir:"*) ;;
    *) echo "$dir is not on your PATH; add it, e.g. in ~/.profile: export PATH=\"$dir:\$PATH\"" ;;
esac
