#!/bin/sh

# Slackinux installer for POSIX-compatible Linux shells.
set -eu

REPOSITORY="Knownassa/Slackinux"
FEED_URL="https://github.com/$REPOSITORY/releases/latest/download/latest.json"
INSTALL_KIND="auto"
DRY_RUN="false"

usage() {
    printf '%s\n' \
        "Install the latest Slackinux release." \
        "" \
        "Usage: install.sh [--auto|--appimage|--deb|--rpm] [--dry-run]" \
        "" \
        "  --auto      Select DEB, RPM, or AppImage for this system (default)" \
        "  --appimage  Install for the current user without root access" \
        "  --deb       Install the Debian/Ubuntu package" \
        "  --rpm       Install the Fedora/RHEL/openSUSE package" \
        "  --dry-run   Verify compatibility and print the selected asset" \
        "  -h, --help  Show this help"
}

die() {
    printf 'Slackinux installer: %s\n' "$*" >&2
    exit 1
}

command_exists() {
    command -v "$1" >/dev/null 2>&1
}

download() {
    source_url=$1
    destination=$2
    if command_exists curl; then
        curl -fsSL --retry 3 --connect-timeout 15 -A "Slackinux installer" \
            "$source_url" -o "$destination"
    elif command_exists wget; then
        wget -q -O "$destination" --timeout=15 --tries=3 "$source_url"
    else
        die "curl or wget is required"
    fi
}

run_as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif command_exists sudo; then
        sudo "$@"
    else
        die "this package installation requires root access or sudo"
    fi
}

finish_dry_run() {
    if [ "$DRY_RUN" = "true" ]; then
        printf 'Compatible package type: %s\nAsset: %s\n' "$INSTALL_KIND" "$1"
        exit 0
    fi
}

asset_url() {
    case $1 in
        deb) filename="Slackinux_${release_version}_amd64.deb" ;;
        rpm) filename="Slackinux-${release_version}-1.x86_64.rpm" ;;
        appimage) filename="Slackinux_${release_version}_amd64.AppImage" ;;
        *) die "unsupported package type: $1" ;;
    esac
    printf 'https://github.com/%s/releases/download/v%s/%s\n' \
        "$REPOSITORY" "$release_version" "$filename"
}

verify_release_asset() {
    package_path=$1
    filename=$2
    checksums="$temporary_dir/SHA256SUMS"
    if [ ! -f "$checksums" ]; then
        if ! download \
                "https://github.com/$REPOSITORY/releases/download/v$release_version/SHA256SUMS" \
                "$checksums"; then
            printf 'Warning: this legacy release has no SHA-256 manifest.\n' >&2
            return 0
        fi
    fi
    expected=$(awk -v name="$filename" '$2 == name { print $1; exit }' "$checksums")
    [ -n "$expected" ] || die "SHA-256 checksum is missing for $filename"
    if command_exists sha256sum; then
        actual=$(sha256sum "$package_path" | awk '{ print $1 }')
    elif command_exists shasum; then
        actual=$(shasum -a 256 "$package_path" | awk '{ print $1 }')
    else
        die "sha256sum or shasum is required to verify the download"
    fi
    [ "$actual" = "$expected" ] || die "SHA-256 verification failed for $filename"
    printf 'Verified SHA-256: %s\n' "$filename"
}

while [ "$#" -gt 0 ]; do
    case $1 in
        --auto) INSTALL_KIND="auto" ;;
        --appimage) INSTALL_KIND="appimage" ;;
        --deb) INSTALL_KIND="deb" ;;
        --rpm) INSTALL_KIND="rpm" ;;
        --dry-run) DRY_RUN="true" ;;
        -h|--help) usage; exit 0 ;;
        *) die "unknown option: $1 (use --help)" ;;
    esac
    shift
done

case $(uname -m) in
    x86_64|amd64) ;;
    *) die "only x86_64 Linux is currently supported" ;;
esac

if [ "$INSTALL_KIND" = "auto" ]; then
    if command_exists dpkg && { command_exists apt || command_exists apt-get; }; then
        INSTALL_KIND="deb"
    elif command_exists rpm && { command_exists dnf || command_exists yum || command_exists zypper; }; then
        INSTALL_KIND="rpm"
    else
        INSTALL_KIND="appimage"
    fi
fi

temporary_dir=$(mktemp -d 2>/dev/null || mktemp -d -t slackinux)
release_json="$temporary_dir/release.json"
trap 'rm -rf "$temporary_dir"' EXIT HUP INT TERM

printf 'Fetching the latest Slackinux release metadata…\n'
download "$FEED_URL" "$release_json"
release_version=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$release_json" | sed -n '1p')
[ -n "$release_version" ] || die "the update feed does not contain a valid version"

case $INSTALL_KIND in
    deb)
        url=$(asset_url deb)
        finish_dry_run "$url"
        package="$temporary_dir/slackinux.deb"
        download "$url" "$package"
        verify_release_asset "$package" "Slackinux_${release_version}_amd64.deb"
        if command_exists apt; then
            run_as_root apt install -y "$package"
        else
            run_as_root apt-get install -y "$package"
        fi
        ;;
    rpm)
        url=$(asset_url rpm)
        finish_dry_run "$url"
        package="$temporary_dir/slackinux.rpm"
        download "$url" "$package"
        verify_release_asset "$package" "Slackinux-${release_version}-1.x86_64.rpm"
        if command_exists dnf; then
            run_as_root dnf install -y "$package"
        elif command_exists yum; then
            run_as_root yum install -y "$package"
        elif command_exists zypper; then
            run_as_root zypper --non-interactive install "$package"
        else
            die "dnf, yum, or zypper is required to install the RPM"
        fi
        ;;
    appimage)
        url=$(asset_url appimage)
        finish_dry_run "$url"
        app_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}/slackinux
        bin_dir="$HOME/.local/bin"
        desktop_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}/applications
        icon_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}/icons/hicolor/512x512/apps
        mkdir -p "$app_dir" "$bin_dir" "$desktop_dir" "$icon_dir"
        staged_appimage="$temporary_dir/Slackinux.AppImage"
        download "$url" "$staged_appimage"
        verify_release_asset "$staged_appimage" "Slackinux_${release_version}_amd64.AppImage"
        chmod 755 "$staged_appimage"
        mv "$staged_appimage" "$app_dir/Slackinux.AppImage"
        ln -sf "$app_dir/Slackinux.AppImage" "$bin_dir/slackinux"
        staged_icon="$temporary_dir/slackinux.png"
        download \
            "https://raw.githubusercontent.com/$REPOSITORY/main/apps/desktop/src-tauri/icons/512x512.png" \
            "$staged_icon"
        mv "$staged_icon" "$icon_dir/slackinux.png"
        desktop_file="$desktop_dir/com.slackinux.desktop"
        printf '%s\n' \
            '[Desktop Entry]' \
            'Type=Application' \
            'Name=Slackinux' \
            'Comment=An unofficial Linux desktop shell for Slack Web' \
            "Exec=\"$app_dir/Slackinux.AppImage\" %U" \
            'Icon=slackinux' \
            'Categories=Network;InstantMessaging;' \
            'MimeType=x-scheme-handler/slack;' \
            'Terminal=false' > "$desktop_file"
        chmod 644 "$desktop_file"
        if command_exists update-desktop-database; then
            update-desktop-database "$desktop_dir" >/dev/null 2>&1 || true
        fi
        if command_exists xdg-mime; then
            xdg-mime default com.slackinux.desktop x-scheme-handler/slack || true
        fi
        printf 'Installed Slackinux for the current user. Run: %s\n' "$bin_dir/slackinux"
        ;;
esac

printf 'Slackinux installation completed successfully.\n'
