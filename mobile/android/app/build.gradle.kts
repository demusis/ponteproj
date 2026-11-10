import java.util.Properties

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
        versionCode = 6
        versionName = "0.5.0"
        // Only the 64-bit ARM phone ABI: keeps the APK small.
        ndk { abiFilters += "arm64-v8a" }
    }

    // The release key lives outside the project, in the owner's profile
    // (`~/.ponte-assinatura/assinatura.properties`, or the file named by
    // PONTE_ASSINATURA): the key file, the file holding its password and the
    // alias. Neither the key nor the password is ever in the repository.
    // Where that folder does not exist (another machine, the checks on
    // GitHub), the build falls back to the debug key and says so.
    val signingFile = file(
        System.getenv("PONTE_ASSINATURA")
            ?: (System.getProperty("user.home") + "/.ponte-assinatura/assinatura.properties")
    )
    val ownKey = signingFile.isFile
    signingConfigs {
        if (ownKey) {
            create("ponte") {
                val props = Properties().apply { signingFile.inputStream().use { load(it) } }
                val secret = file(props.getProperty("storePasswordFile")).readText().trim()
                storeFile = file(props.getProperty("storeFile"))
                storeType = "PKCS12"
                storePassword = secret
                keyAlias = props.getProperty("keyAlias")
                keyPassword = secret
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = if (ownKey) {
                signingConfigs.getByName("ponte")
            } else {
                logger.warn("Ponte de bolso: chave própria não encontrada; APK assinado com a chave de depuração.")
                signingConfigs.getByName("debug")
            }
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
