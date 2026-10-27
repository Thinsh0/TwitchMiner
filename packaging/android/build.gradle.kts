// See build-apk.sh, which builds the Rust library first and signs the result.
plugins {
    id("com.android.application") version "8.13.2"
    id("com.google.gms.google-services") version "4.4.4" apply false
}

// Firebase (notifications pushed by the server) only when the project's configuration
// is there: google-services.json, written by build-apk.sh from $GOOGLE_SERVICES_JSON.
// Without it the app works the same, minus those notifications.
if (file("google-services.json").exists()) {
    apply(plugin = "com.google.gms.google-services")
}

val root = rootDir.resolve("../..")
layout.buildDirectory.set(root.resolve("target/android"))

android {
    namespace = "io.github.thinsh0.twitchminer"
    compileSdk = (findProperty("tm.compileSdk") as String? ?: "35").toInt()

    defaultConfig {
        applicationId = "io.github.thinsh0.twitchminer"
        minSdk = 26
        targetSdk = 34
        versionCode = (findProperty("tm.versionCode") as String? ?: "1").toInt()
        versionName = findProperty("tm.versionName") as String? ?: "0.0.0"
        // Only the architectures the Rust library is built for (libraries pulled in
        // by Firebase exist for others).
        ndk {
            abiFilters += (findProperty("tm.abis") as String? ?: "arm64-v8a").split(" ")
        }
    }

    sourceSets["main"].apply {
        manifest.srcFile("AndroidManifest.xml")
        java.srcDirs("java")
        res.srcDirs("res")
        jniLibs.srcDirs(root.resolve("target/android-jni"))
    }

    buildTypes {
        release {
            // Signed afterwards by build-apk.sh (apksigner).
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    // Compressed in the APK: a much smaller download.
    packaging {
        jniLibs.useLegacyPackaging = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    lint {
        checkReleaseBuilds = false
    }
}

dependencies {
    implementation(platform("com.google.firebase:firebase-bom:34.19.0"))
    implementation("com.google.firebase:firebase-messaging")
}
