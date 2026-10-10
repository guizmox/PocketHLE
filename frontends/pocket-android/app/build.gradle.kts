plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.pockethle.app"
    compileSdk = 35
    buildToolsVersion = "35.0.0"
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "com.pockethle.app"
        minSdk = 24
        targetSdk = 34
        versionCode = 4
        versionName = "0.3.1-android-test2"

        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
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

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.activity:activity-ktx:1.9.0")
    implementation("androidx.fragment:fragment-ktx:1.7.1")
    implementation("androidx.recyclerview:recyclerview:1.3.2")
    implementation("androidx.preference:preference-ktx:1.2.1")
    implementation("androidx.constraintlayout:constraintlayout:2.1.4")
    implementation("com.google.android.material:material:1.12.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.0")
    implementation("org.json:json:20240303")
}

// A missing native bridge must fail packaging, rather than produce an APK that cannot launch.
val verifyNativeLibraries by tasks.registering {
    doLast {
        for (abi in listOf("arm64-v8a", "armeabi-v7a")) {
            check(file("src/main/jniLibs/$abi/libpockethle_jni.so").isFile) {
                "Missing $abi native bridge. From repository root: bash tools/build-android-native.sh"
            }
        }
    }
}
tasks.named("preBuild").configure { dependsOn(verifyNativeLibraries) }
