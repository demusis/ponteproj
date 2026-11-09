plugins {
    id("com.android.application") version "8.5.2" apply false
    id("org.jetbrains.kotlin.android") version "1.9.24" apply false
    // Renders the screens to PNG files on the computer (unit tests, no phone needed).
    id("app.cash.paparazzi") version "1.3.4" apply false
}
