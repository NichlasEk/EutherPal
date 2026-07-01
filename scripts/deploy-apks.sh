#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REMOTE="${EUTHERPAL_DEPLOY_HOST:-euther-server}"
REMOTE_ROOT="${EUTHERPAL_REMOTE_ROOT:-/home/nichlas/EutherPal}"
TV_APK="$ROOT/android-tv/dist/eutherpal-tv.apk"
MOBILE_APK="$ROOT/android-mobile/dist/eutherpal-mobile.apk"

cd "$ROOT"

echo "Building signed APKs locally..."
"$ROOT/android-tv/build-apk.sh"
"$ROOT/android-mobile/build-apk.sh"

LOCAL_TV_SHA="$(sha256sum "$TV_APK" | awk '{print $1}')"
LOCAL_MOBILE_SHA="$(sha256sum "$MOBILE_APK" | awk '{print $1}')"

echo "Uploading APKs to $REMOTE..."
scp "$TV_APK" "$MOBILE_APK" "$REMOTE:/tmp/"

echo "Installing APKs into remote EutherPal dist paths..."
ssh -tt "$REMOTE" "cp /tmp/eutherpal-tv.apk '$REMOTE_ROOT/android-tv/dist/eutherpal-tv.apk' && cp /tmp/eutherpal-mobile.apk '$REMOTE_ROOT/android-mobile/dist/eutherpal-mobile.apk' && sha256sum '$REMOTE_ROOT/android-tv/dist/eutherpal-tv.apk' '$REMOTE_ROOT/android-mobile/dist/eutherpal-mobile.apk'"

echo "Verifying EutherOxide download routes..."
REMOTE_TV_SHA="$(ssh "$REMOTE" "curl -fsS http://127.0.0.1:8080/downloads/EutherPalTV-release-signed.apk | sha256sum | cut -d' ' -f1")"
REMOTE_MOBILE_SHA="$(ssh "$REMOTE" "curl -fsS http://127.0.0.1:8080/downloads/EutherPalMobile-release-signed.apk | sha256sum | cut -d' ' -f1")"

if [[ "$LOCAL_TV_SHA" != "$REMOTE_TV_SHA" ]]; then
  echo "TV APK hash mismatch: local=$LOCAL_TV_SHA remote=$REMOTE_TV_SHA" >&2
  exit 1
fi

if [[ "$LOCAL_MOBILE_SHA" != "$REMOTE_MOBILE_SHA" ]]; then
  echo "Mobile APK hash mismatch: local=$LOCAL_MOBILE_SHA remote=$REMOTE_MOBILE_SHA" >&2
  exit 1
fi

cat <<EOF
APK deploy complete.
TV:     http://192.168.32.186:8080/downloads/EutherPalTV-release-signed.apk
Mobile: http://192.168.32.186:8080/downloads/EutherPalMobile-release-signed.apk
TV SHA: $LOCAL_TV_SHA
Mobile SHA: $LOCAL_MOBILE_SHA
EOF
