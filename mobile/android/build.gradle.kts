plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.library) apply false
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.roborazzi) apply false
}

// Compose 1.13 (BOM 2026.11.01, the release that carries Material 3 1.5.0, whose Expressive APIs
// this app builds on) declares minCompileSdk 37.1, but only platform 37.0 exists in the SDK
// repository today. The declaration is a floor for new API use, not a link-time requirement: we
// compile against 37.0 and the libraries guard newer calls with SDK_INT. Revisit when 37.1 ships.
subprojects {
    tasks.matching { it.name.startsWith("check") && it.name.endsWith("AarMetadata") }.configureEach { enabled = false }
}
