#!/bin/bash
# Builds the Android prototype: the Rust core (aarch64) and the debug APK.
# Toolchain: the portable one installed next to the Ponte devtools.
set -e
DEV=/c/Users/CARLO/AppData/Local/Programs/PonteProj/devtools
A=$DEV/android
NDK=$A/sdk/ndk/26.3.11579264/toolchains/llvm/prebuilt/windows-x86_64/bin
REPO="/h/Meu Drive/projetos/ponteproj"

export CARGO_HOME="$(cygpath -w "$DEV/cargo")" RUSTUP_HOME="$(cygpath -w "$DEV/rustup")"
export RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-gnu
export PATH="$DEV/cargo/bin:$DEV/rustup/toolchains/$RUSTUP_TOOLCHAIN/bin:$PATH"
# Host build scripts (build.rs of dependencies) still link for Windows.
RL="$DEV/rustup/toolchains/$RUSTUP_TOOLCHAIN/lib/rustlib/x86_64-pc-windows-gnu/bin"
export PATH="$DEV/mingw/w64devkit/bin:$RL:$RL/self-contained:$PATH"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$(cygpath -w "$RL/self-contained/x86_64-w64-mingw32-gcc.exe")"
export CC_x86_64_pc_windows_gnu="$(cygpath -w "$DEV/mingw/w64devkit/bin/gcc.exe")"
export AR_x86_64_pc_windows_gnu="$(cygpath -w "$DEV/mingw/w64devkit/bin/ar.exe")"
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
export JAVA_HOME="$(cygpath -w "$A/jdk")" ANDROID_HOME="$(cygpath -w "$A/sdk")"
export PATH="$A/jdk/bin:$PATH"
cd "$REPO/mobile/android"
printf 'sdk.dir=%s\n' "$(cygpath -m "$A/sdk")" > local.properties
"$A/gradle/bin/gradle" --no-daemon assembleDebug
ls -la app/build/outputs/apk/debug/*.apk
