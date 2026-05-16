plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

val buildNativeGsplat by tasks.registering(Exec::class) {
    workingDir = rootProject.projectDir.parentFile
    commandLine("bash", rootProject.file("scripts/build-native.sh").absolutePath)
}

android {
    namespace = "com.gsplat.android"
    compileSdk = 35

    defaultConfig {
        minSdk = 24

        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
        }
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

tasks.matching { it.name == "preBuild" }.configureEach {
    dependsOn(buildNativeGsplat)
}
