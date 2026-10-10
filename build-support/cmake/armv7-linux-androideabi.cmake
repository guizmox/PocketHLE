set(ANDROID_ABI "armeabi-v7a" CACHE STRING "" FORCE)
set(ANDROID_PLATFORM android-24 CACHE STRING "" FORCE)
set(ANDROID_STL c++_static CACHE STRING "" FORCE)
include("$ENV{ANDROID_NDK_HOME}/build/cmake/android.toolchain.cmake")
