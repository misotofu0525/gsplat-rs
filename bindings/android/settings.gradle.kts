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

rootProject.name = "gsplat-android"
include(":gsplat-android")
include(":host-smoke")
include(":sample-app")

project(":sample-app").projectDir = file("../../examples/android/app")
