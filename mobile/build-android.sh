#!/bin/bash
# Builds Ponte de bolso: the Rust core (aarch64) and the APK (shrunk; signed
# with the project's own key when ~/.ponte-assinatura exists, else the debug key).
#
# Two kinds of machine:
# - with the portable Ponte devtools (DEV below, or PONTE_DEVTOOLS): uses them;
# - without them: uses what is installed (cargo/rustup on PATH, ANDROID_HOME,
#   JAVA_HOME, the newest NDK under the SDK, gradle on PATH or PONTE_GRADLE).
set -e
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEV="${PONTE_DEVTOOLS:-/c/Users/CARLO/AppData/Local/Programs/PonteProj/devtools}"

if [ -d "$DEV" ]; then
    A=$DEV/android
    SDK=$A/sdk
    JDK=$A/jdk
    GRADLE="$A/gradle/bin/gradle"
    NDK=$SDK/ndk/26.3.11579264/toolchains/llvm/prebuilt/windows-x86_64/bin
    export CARGO_HOME="$(cygpath -w "$DEV/cargo")" RUSTUP_HOME="$(cygpath -w "$DEV/rustup")"
    export RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-gnu
    export PATH="$DEV/cargo/bin:$DEV/rustup/toolchains/$RUSTUP_TOOLCHAIN/bin:$PATH"
    # Host build scripts (build.rs of dependencies) still link for Windows.
    RL="$DEV/rustup/toolchains/$RUSTUP_TOOLCHAIN/lib/rustlib/x86_64-pc-windows-gnu/bin"
    export PATH="$DEV/mingw/w64devkit/bin:$RL:$RL/self-contained:$PATH"
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$(cygpath -w "$RL/self-contained/x86_64-w64-mingw32-gcc.exe")"
    export CC_x86_64_pc_windows_gnu="$(cygpath -w "$DEV/mingw/w64devkit/bin/gcc.exe")"
    export AR_x86_64_pc_windows_gnu="$(cygpath -w "$DEV/mingw/w64devkit/bin/ar.exe")"
else
    : "${ANDROID_HOME:?ANDROID_HOME não definido (pasta do Android SDK)}"
    : "${JAVA_HOME:?JAVA_HOME não definido (JDK 17)}"
    SDK="$(cygpath -u "$ANDROID_HOME")"
    JDK="$(cygpath -u "$JAVA_HOME")"
    # Gradle: PONTE_GRADLE, else the one on PATH, else the newest in the wrapper cache.
    GRADLE="${PONTE_GRADLE:-$(command -v gradle || ls -d "$HOME"/.gradle/wrapper/dists/gradle-*/*/gradle-*/bin/gradle 2>/dev/null | sort -V | tail -1)}"
    NDK_ROOT="$(ls -d "$SDK"/ndk/*/ 2>/dev/null | sort -V | tail -1)"
    [ -n "$NDK_ROOT" ] || { echo "Nenhum NDK em $SDK/ndk." >&2; exit 1; }
    NDK="${NDK_ROOT%/}/toolchains/llvm/prebuilt/windows-x86_64/bin"
fi
unset CARGO_ENCODED_RUSTFLAGS CC AR
# The phone: NDK clang for API 26.
export CC_aarch64_linux_android="$(cygpath -w "$NDK/aarch64-linux-android26-clang.cmd")"
export AR_aarch64_linux_android="$(cygpath -w "$NDK/llvm-ar.exe")"
# The .cmd wrapper breaks on paths with spaces ("Meu Drive"): clang itself
# links, told the target and API level.
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$(cygpath -w "$NDK/clang.exe")"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=--target=aarch64-linux-android26"
# Build outside the synced folder: faster, and no spaces in object paths.
export CARGO_TARGET_DIR="$(cygpath -w "$HOME/AppData/Local/Temp/ponte-mobile-target")"

echo "== Rust core (aarch64-linux-android)"
cd "$REPO/mobile/rust"
cargo build --release --target aarch64-linux-android
mkdir -p "$REPO/mobile/android/app/src/main/jniLibs/arm64-v8a"
cp "$(cygpath -u "$CARGO_TARGET_DIR")/aarch64-linux-android/release/libponte_mobile.so" "$REPO/mobile/android/app/src/main/jniLibs/arm64-v8a/"
ls -la "$REPO/mobile/android/app/src/main/jniLibs/arm64-v8a/libponte_mobile.so"

if [ "$1" = "--rust-only" ]; then exit 0; fi

echo "== APK"
[ -n "$GRADLE" ] || { echo "Gradle não encontrado: ponha no PATH ou defina PONTE_GRADLE." >&2; exit 1; }
export JAVA_HOME="$(cygpath -w "$JDK")" ANDROID_HOME="$(cygpath -w "$SDK")"
export PATH="$JDK/bin:$PATH"
cd "$REPO/mobile/android"
printf 'sdk.dir=%s\n' "$(cygpath -m "$SDK")" > local.properties
"$GRADLE" --no-daemon assembleRelease
ls -la app/build/outputs/apk/release/*.apk
