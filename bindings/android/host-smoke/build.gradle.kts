import org.gradle.process.CommandLineArgumentProvider
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    kotlin("jvm")
    application
}

application {
    mainClass.set("com.gsplat.demo.GsplatJniSmoke")
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

tasks.named<JavaExec>("run") {
    val jniLibPath = providers.gradleProperty("gsplatJniLibPath")
    val ffiLibPath = providers.gradleProperty("gsplatFfiLibPath")
    val datasetPath = providers.gradleProperty("gsplatDatasetPath")

    jvmArgs("--enable-native-access=ALL-UNNAMED")
    jvmArgumentProviders.add(CommandLineArgumentProvider {
        val libraryPath = "${jniLibPath.get()}:${ffiLibPath.get()}"
        listOf("-Djava.library.path=$libraryPath")
    })

    environment("DYLD_LIBRARY_PATH", "${jniLibPath.get()}:${ffiLibPath.get()}")
    environment("LD_LIBRARY_PATH", "${jniLibPath.get()}:${ffiLibPath.get()}")
    args(datasetPath.get())
}
