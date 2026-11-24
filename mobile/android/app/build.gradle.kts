import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("app.cash.paparazzi")
}

// The release key lives outside the project, in the owner's profile
// (`~/.ponte-assinatura/assinatura.properties`, or the file named by
// PONTE_ASSINATURA): the key file, the file holding its password and the
// alias. Neither the key nor the password is ever in the repository.
// Where that folder does not exist, packaging a release FAILS, unless
// `-Pponte.semAssinatura=true` is passed (the APK then gets the debug key).
// Compiling and unit tests of the release variant never need the key.
// Resolved ONCE, relative to this project like Gradle's `file(...)`, and used
// both by the signing configuration and by the check below, so they cannot
// disagree about which file counts (a relative PONTE_ASSINATURA included).
val signingFile: File = file(
    System.getenv("PONTE_ASSINATURA")
        ?: (System.getProperty("user.home") + "/.ponte-assinatura/assinatura.properties")
)
val ownKey: Boolean = signingFile.isFile

android {
    namespace = "io.github.demusis.ponte"
    compileSdk = 34

    defaultConfig {
        applicationId = "io.github.demusis.ponte"
        minSdk = 26
        targetSdk = 34
        versionCode = 42
        versionName = "0.28.0"
        // Only the 64-bit ARM phone ABI: keeps the APK small.
        ndk { abiFilters += "arm64-v8a" }
    }

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
                // Only reached when packaging is allowed (see the check below).
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

// A release that would silently carry the debug key is refused: only the tasks
// that sign or package the release variant check, so testReleaseUnitTest and
// compilation still work on a machine without the key.
val semAssinatura = providers.gradleProperty("ponte.semAssinatura").orNull == "true"
val chaveProprianaExiste = ownKey
gradle.taskGraph.whenReady {
    val empacota = allTasks.any {
        it.project == project && it.name in setOf("validateSigningRelease", "packageRelease", "assembleRelease", "bundleRelease")
    }
    if (empacota && !chaveProprianaExiste) {
        if (semAssinatura) {
            logger.warn("Ponte de bolso: SEM chave própria (-Pponte.semAssinatura=true); APK assinado com a chave de depuração.")
        } else {
            throw GradleException(
                "Ponte de bolso: chave de assinatura da versão não encontrada " +
                    "(~/.ponte-assinatura/assinatura.properties ou PONTE_ASSINATURA). " +
                    "Para gerar de propósito um APK com a chave de depuração, passe -Pponte.semAssinatura=true."
            )
        }
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.06.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    testImplementation("junit:junit:4.13.2")
}
