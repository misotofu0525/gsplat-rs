plugins {
    id("com.android.application")
}

android {
    namespace = "com.gsplat.competitive.playcanvas"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.gsplat.competitive.playcanvas"
        minSdk = 29
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        debug {
            isDebuggable = true
            isMinifyEnabled = false
        }
        release {
            isDebuggable = false
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
