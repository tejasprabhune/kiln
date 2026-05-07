#!/bin/sh
# Install the latest kiln release binary.
# Supports Linux (x86_64, aarch64) and macOS (x86_64, Apple Silicon).
# Linux x86_64 uses the statically linked musl build, which works on any
# kernel regardless of glibc version (HPC nodes, GPU instances, old distros).
set -eu

REPO="tejasprabhune/kiln"
BIN_NAME="kiln"

# Allow override via env var (useful for CI or custom setups).
BIN_DIR="${KILN_BIN_DIR:-${KILN_INSTALL_DIR:-}}"
NO_MODIFY_PATH="${KILN_NO_MODIFY_PATH:-0}"
# Set KILN_VERSION to skip the GitHub API call and install a specific tag.
KILN_VERSION="${KILN_VERSION:-}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

say() {
    echo "kiln-installer: $*"
}

err() {
    echo "kiln-installer: error: $*" >&2
    exit 1
}

need_cmd() {
    command -v "$1" > /dev/null 2>&1 || err "required command not found: $1"
}

check_cmd() {
    command -v "$1" > /dev/null 2>&1
}

# Download $1 to file $2, using curl or wget.
download() {
    _url="$1"
    _dest="$2"

    if check_cmd curl; then
        curl -sSfL "$_url" -o "$_dest"
    elif check_cmd wget; then
        wget -q "$_url" -O "$_dest"
    else
        err "neither curl nor wget found; cannot download kiln"
    fi
}

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

get_target() {
    _os="$(uname -s)"
    _arch="$(uname -m)"

    case "$_os" in
        Linux)
            case "$_arch" in
                x86_64)  echo "x86_64-unknown-linux-musl" ;;
                aarch64) echo "aarch64-unknown-linux-musl" ;;
                *)       err "unsupported Linux architecture: $_arch" ;;
            esac
            ;;
        Darwin)
            # sysctl doesn't lie even under Rosetta 2, unlike uname -m.
            if sysctl hw.optional.arm64 2>/dev/null | grep -q ': 1'; then
                echo "aarch64-apple-darwin"
            elif [ "$_arch" = "arm64" ]; then
                echo "aarch64-apple-darwin"
            else
                echo "x86_64-apple-darwin"
            fi
            ;;
        *)
            err "unsupported OS: $_os. Install with: cargo install kiln-sv"
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Install directory resolution (mirrors uv's priority order)
# ---------------------------------------------------------------------------

resolve_bin_dir() {
    if [ -n "$BIN_DIR" ]; then
        echo "$BIN_DIR"
        return
    fi
    if [ -n "${XDG_BIN_HOME:-}" ]; then
        echo "$XDG_BIN_HOME"
        return
    fi
    if [ -n "${XDG_DATA_HOME:-}" ]; then
        echo "$XDG_DATA_HOME/../bin"
        return
    fi
    echo "${HOME}/.local/bin"
}

# ---------------------------------------------------------------------------
# PATH management
# ---------------------------------------------------------------------------

# Write an idempotent env snippet to $1 and source it from $2 (an rc file).
add_to_path() {
    _bin_dir="$1"
    _rc="$2"
    _env_script="${_bin_dir}/env"

    # Write the env script if it doesn't exist yet.
    if [ ! -f "$_env_script" ]; then
        cat > "$_env_script" <<EOF
#!/bin/sh
case ":\${PATH}:" in
    *:"${_bin_dir}":*) ;;
    *) export PATH="${_bin_dir}:\$PATH" ;;
esac
EOF
    fi

    # Add a source line to the rc file if not already there.
    if [ -f "$_rc" ] && ! grep -qF "$_env_script" "$_rc" 2>/dev/null; then
        printf '\n. "%s"\n' "$_env_script" >> "$_rc"
        return 0  # modified
    fi
    return 1  # already present or file missing
}

configure_path() {
    _bin_dir="$1"
    _modified=0

    # If _bin_dir is already on PATH, nothing to do.
    case ":${PATH}:" in
        *:"${_bin_dir}":*)
            NO_MODIFY_PATH=1
            ;;
    esac

    if [ "$NO_MODIFY_PATH" = "1" ]; then
        return
    fi

    # Write to GITHUB_PATH if we're in a GitHub Actions runner.
    if [ -n "${GITHUB_PATH:-}" ]; then
        echo "$_bin_dir" >> "$GITHUB_PATH"
        return
    fi

    # Try common rc files.
    for _rc in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc"; do
        if add_to_path "$_bin_dir" "$_rc"; then
            _modified=1
        fi
    done

    # Fish shell — only configure if fish is actually in PATH.
    _fish_dir="$HOME/.config/fish/conf.d"
    if check_cmd fish && [ -d "$_fish_dir" ]; then
        _fish_rc="$_fish_dir/kiln.env.fish"
        if [ ! -f "$_fish_rc" ]; then
            cat > "$_fish_rc" <<EOF
if not contains "${_bin_dir}" \$PATH
    set -x PATH "${_bin_dir}" \$PATH
end
EOF
            _modified=1
        fi
    fi

    if [ "$_modified" = "1" ]; then
        say "added ${_bin_dir} to PATH in your shell rc files"
        say "restart your shell or run:  . \"${_bin_dir}/env\""
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
    need_cmd uname
    need_cmd mktemp
    need_cmd chmod
    need_cmd mkdir
    need_cmd mv
    need_cmd grep
    need_cmd tar

    _target="$(get_target)"
    _bin_dir="$(resolve_bin_dir)"

    # Resolve release tag: use KILN_VERSION if set, otherwise query GitHub API.
    if [ -n "$KILN_VERSION" ]; then
        _latest="$KILN_VERSION"
    else
        _api_url="https://api.github.com/repos/${REPO}/releases/latest"
        _tmp_json="$(mktemp)"
        # Pass GITHUB_TOKEN if available to avoid rate limiting.
        if [ -n "${GITHUB_TOKEN:-}" ]; then
            if check_cmd curl; then
                curl -sSfL -H "Authorization: Bearer ${GITHUB_TOKEN}" "$_api_url" -o "$_tmp_json"
            elif check_cmd wget; then
                wget -q --header="Authorization: Bearer ${GITHUB_TOKEN}" "$_api_url" -O "$_tmp_json"
            fi
        else
            download "$_api_url" "$_tmp_json"
        fi
        _latest="$(grep '"tag_name"' "$_tmp_json" | sed 's/.*"tag_name": *"\(.*\)".*/\1/')"
        rm -f "$_tmp_json"

        if [ -z "$_latest" ]; then
            err "GitHub API rate limit hit or could not determine latest release; set GITHUB_TOKEN or specify a version with KILN_VERSION=<tag>"
        fi
    fi

    # Release artefacts are tarballs of the form
    # kiln-${tag}-${target}.tar.gz with a single `kiln` binary inside.
    _url="https://github.com/${REPO}/releases/download/${_latest}/kiln-${_latest}-${_target}.tar.gz"
    _dest="${_bin_dir}/${BIN_NAME}"

    say "installing kiln ${_latest} (${_target}) to ${_dest}"
    mkdir -p "$_bin_dir"

    # Stage the download in a temp dir alongside the bin dir so the final
    # mv is within one filesystem (avoids cross-device mv failures).
    _tmp_dir="$(mktemp -d "${_bin_dir}/kiln.tmp.XXXXXX")"
    trap 'rm -rf "$_tmp_dir"' EXIT

    _archive="${_tmp_dir}/kiln.tar.gz"
    download "$_url" "$_archive"
    tar -xzf "$_archive" -C "$_tmp_dir"

    if [ ! -f "${_tmp_dir}/${BIN_NAME}" ]; then
        err "expected ${BIN_NAME} inside ${_url}, but the archive did not contain it"
    fi
    chmod +x "${_tmp_dir}/${BIN_NAME}"

    # Atomic replace: move into place only after successful extraction.
    mv "${_tmp_dir}/${BIN_NAME}" "$_dest"

    # Verify the installed binary actually runs.
    if ! "${_dest}" --version > /dev/null 2>&1; then
        rm -f "$_dest"
        err "installed binary failed to run (wrong architecture or corrupt download): ${_dest}"
    fi

    say "installed: $("${_dest}" --version)"

    configure_path "$_bin_dir"
}

main "$@"
