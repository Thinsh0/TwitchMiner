// The Android application: Java glue (java/), resources (res/) and the Rust library
// built beforehand by build-apk.sh. Built with `./gradlew assembleRelease` from there.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "twitchminer"
