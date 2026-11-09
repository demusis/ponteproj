plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "io.github.demusis.ponte"
    compileSdk = 34

    defaultConfig {
        applicationId = "io.github.demusis.ponte"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
        // Only the 64-bit ARM phone ABI: keeps the APK small.
        ndk { abiFilters += "arm64-v8a" }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}
// No dependencies on purpose: plain Activity, no AndroidX (the Kotlin stdlib is added by the plugin).
