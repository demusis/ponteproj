plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("app.cash.paparazzi")
}

android {
    namespace = "io.github.demusis.ponte"
    compileSdk = 34

    defaultConfig {
        applicationId = "io.github.demusis.ponte"
        minSdk = 26
        targetSdk = 34
        versionCode = 3
        versionName = "0.3.0"
        // Only the 64-bit ARM phone ABI: keeps the APK small.
        ndk { abiFilters += "arm64-v8a" }
    }

    buildTypes {
        release {
            // Shrunk and optimized, but signed with the same (debug) key as the earlier
            // versions, so it installs over them without losing the local library.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    buildFeatures { compose = true }
    composeOptions { kotlinCompilerExtensionVersion = "1.5.14" }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.06.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.activity:activity-compose:1.9.0")
    testImplementation("junit:junit:4.13.2")
}
