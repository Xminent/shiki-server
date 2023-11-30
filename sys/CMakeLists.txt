cmake_minimum_required(VERSION 3.16)

project(sys)

list(APPEND CMAKE_MODULE_PATH ${CMAKE_BINARY_DIR})
list(APPEND CMAKE_PREFIX_PATH ${CMAKE_BINARY_DIR})

include(cmake/conan.cmake)

conan_cmake_configure(
    REQUIRES
    opus/1.4
    opusfile/0.12
    ogg/1.3.5
    GENERATORS
    cmake_find_package
    IMPORTS
    "bin, *.dll -> ./bin"
    "lib, *.dylib* -> ./bin"
    "lib, *.so -> ./bin"
    "lib, *.a -> ./lib"
    "lib, *.lib -> ./lib"
    "include, * -> ./include"
    OPTIONS
    opus:shared=False
    opusfile:shared=False
)
conan_cmake_autodetect(settings)
conan_cmake_install(
    PATH_OR_REFERENCE
    .
    BUILD
    missing
    REMOTE
    conancenter
    SETTINGS
    ${settings}
)

install(DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/lib DESTINATION .)
install(DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/include DESTINATION .)
install(FILES wrapper.h DESTINATION .)
