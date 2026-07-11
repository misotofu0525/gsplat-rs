plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

fun gitOutput(vararg arguments: String): String =
    providers.exec {
        workingDir(rootProject.projectDir.resolve("../.."))
        commandLine("git", *arguments)
        isIgnoreExitValue = true
    }.standardOutput.asText.get().trim()

val repositoryCommit = gitOutput("rev-parse", "HEAD").ifEmpty { "unknown" }
val repositoryDirty = gitOutput("status", "--porcelain").isNotEmpty()

android {
    namespace = "com.gsplat.example"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.gsplat.example"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.3"
        buildConfigField("String", "REPOSITORY_COMMIT", "\"$repositoryCommit\"")
        buildConfigField("boolean", "REPOSITORY_DIRTY", repositoryDirty.toString())

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

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    sourceSets {
        getByName("main") {
            assets.srcDir(layout.buildDirectory.dir("generated/showcase-assets"))
        }
    }

    androidResources {
        noCompress += "ply"
    }

    buildFeatures {
        buildConfig = true
    }
}

dependencies {
    implementation(project(":gsplat-android"))
    testImplementation("junit:junit:4.13.2")
}
