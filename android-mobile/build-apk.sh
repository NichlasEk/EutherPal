#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SDK="${ANDROID_HOME:-/opt/android-sdk}"
BUILD_TOOLS="${ANDROID_BUILD_TOOLS:-$SDK/build-tools/36.0.0}"
PLATFORM="${ANDROID_PLATFORM:-$SDK/platforms/android-34/android.jar}"
BUILD="$ROOT/build"
DIST="$ROOT/dist"
KEYSTORE="$ROOT/.keystore/eutherpal-mobile-dev.jks"
ALIAS="${EUTHERPAL_KEY_ALIAS:-eutherpal-mobile}"
STOREPASS="${EUTHERPAL_KEYSTORE_PASS:-android}"
KEYPASS="${EUTHERPAL_KEY_PASS:-$STOREPASS}"

mkdir -p "$BUILD"/res "$BUILD"/gen "$BUILD"/classes "$BUILD"/dex "$DIST" "$(dirname "$KEYSTORE")"
rm -rf "$BUILD"/res "$BUILD"/gen "$BUILD"/classes "$BUILD"/dex
mkdir -p "$BUILD"/res "$BUILD"/gen "$BUILD"/classes "$BUILD"/dex

"$BUILD_TOOLS/aapt2" compile --dir "$ROOT/res" -o "$BUILD/res/resources.zip"
"$BUILD_TOOLS/aapt2" link \
  -I "$PLATFORM" \
  --manifest "$ROOT/AndroidManifest.xml" \
  --java "$BUILD/gen" \
  --auto-add-overlay \
  -o "$BUILD/unsigned.apk" \
  "$BUILD/res/resources.zip"

javac -source 8 -target 8 \
  -bootclasspath "$PLATFORM" \
  -classpath "$BUILD/gen" \
  -d "$BUILD/classes" \
  $(find "$ROOT/src" "$BUILD/gen" -name '*.java' | sort)

"$BUILD_TOOLS/d8" \
  --classpath "$PLATFORM" \
  --min-api 23 \
  --output "$BUILD/dex" \
  $(find "$BUILD/classes" -name '*.class' | sort)

cp "$BUILD/unsigned.apk" "$BUILD/with-dex.apk"
(
  cd "$BUILD/dex"
  zip -q "$BUILD/with-dex.apk" classes.dex
)

"$BUILD_TOOLS/zipalign" -f -p 4 "$BUILD/with-dex.apk" "$BUILD/aligned.apk"

if [ ! -f "$KEYSTORE" ]; then
  keytool -genkeypair \
    -keystore "$KEYSTORE" \
    -storepass "$STOREPASS" \
    -keypass "$KEYPASS" \
    -alias "$ALIAS" \
    -keyalg RSA \
    -keysize 2048 \
    -validity 10000 \
    -dname "CN=EutherPal Mobile Dev,O=ApothicTech,C=SE"
fi

"$BUILD_TOOLS/apksigner" sign \
  --ks "$KEYSTORE" \
  --ks-key-alias "$ALIAS" \
  --ks-pass "pass:$STOREPASS" \
  --key-pass "pass:$KEYPASS" \
  --out "$DIST/eutherpal-mobile.apk" \
  "$BUILD/aligned.apk"

"$BUILD_TOOLS/apksigner" verify --verbose "$DIST/eutherpal-mobile.apk"
echo "$DIST/eutherpal-mobile.apk"
