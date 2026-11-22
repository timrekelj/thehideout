// Generated protobuf-javalite messages and connect-kotlin clients (mobile/api/generate.sh).
// A plain JVM library on purpose: :core stays testable on the JVM without Android.
plugins {
    alias(libs.plugins.kotlin.jvm)
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        // Generated code: deprecations and the like are not ours to fix.
        suppressWarnings.set(true)
    }
}

tasks.withType<JavaCompile>().configureEach {
    options.release.set(17)
    options.compilerArgs.add("-Xlint:none")
}

dependencies {
    api(libs.protobuf.javalite)
    api(libs.protobuf.kotlin.lite)
    api(libs.connect.kotlin)
    api(libs.connect.kotlin.okhttp)
    api(libs.connect.kotlin.google.javalite)
    api(libs.okhttp)
}
