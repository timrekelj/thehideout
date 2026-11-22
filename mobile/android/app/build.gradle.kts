// The application: MainActivity, AppContainer, Keystore token store, manifest and icons.
plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "si.zabit.thehideout"
    compileSdk = 37

    defaultConfig {
        applicationId = "si.zabit.thehideout"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
    }

    buildFeatures { buildConfig = true }

    buildTypes {
        debug {
            // The emulator reaches the host machine at 10.0.2.2; the dev server listens on :8080.
            buildConfigField("String", "DEFAULT_SERVER", "\"http://10.0.2.2:8080\"")
        }
        release {
            // A placeholder: the sign-in phase adds a server field. Release builds are HTTPS-only.
            buildConfigField("String", "DEFAULT_SERVER", "\"https://hideout.example.com\"")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        allWarningsAsErrors.set(true)
    }
}

dependencies {
    implementation(platform(libs.compose.bom))
    implementation(project(":core"))
    implementation(project(":ui"))
    implementation(project(":design"))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.kotlinx.coroutines.android)
}
