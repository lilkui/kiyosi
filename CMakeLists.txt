cmake_minimum_required(VERSION 3.25)

project(ito VERSION 0.1.0 LANGUAGES CXX)

include(CMakePackageConfigHelpers)
include(CTest)
include(GNUInstallDirs)

option(ITO_BUILD_SHARED "Build the optional shared library" OFF)

set(ITO_SOURCES src/ito.cpp)

add_library(ito STATIC ${ITO_SOURCES})
add_library(ito::ito ALIAS ito)
target_compile_features(ito PUBLIC cxx_std_23)
target_include_directories(ito
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
)
set_target_properties(ito PROPERTIES EXPORT_NAME ito)

if(ITO_BUILD_SHARED)
    add_library(ito_shared SHARED ${ITO_SOURCES})
    add_library(ito::ito_shared ALIAS ito_shared)
    target_compile_features(ito_shared PUBLIC cxx_std_23)
    target_include_directories(ito_shared
        PUBLIC
            $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
            $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
    )
    set_target_properties(ito_shared PROPERTIES EXPORT_NAME ito_shared)
endif()

if(BUILD_TESTING)
    include(FetchContent)
    FetchContent_Declare(
        Catch2
        GIT_REPOSITORY https://github.com/catchorg/Catch2.git
        GIT_TAG 56809e5282f104c5c8b570e7c2996cdc352d94f1
    )
    FetchContent_MakeAvailable(Catch2)

    add_executable(ito_tests tests/smoke_tests.cpp tests/parity_fixture_tests.cpp)
    target_link_libraries(ito_tests PRIVATE ito Catch2::Catch2WithMain)
    target_compile_features(ito_tests PRIVATE cxx_std_23)
    target_compile_definitions(ito_tests PRIVATE ITO_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}")

    list(APPEND CMAKE_MODULE_PATH ${catch2_SOURCE_DIR}/extras)
    include(Catch)
    catch_discover_tests(ito_tests)
endif()

install(TARGETS ito
    EXPORT itoTargets
    ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
    INCLUDES DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}
)
if(ITO_BUILD_SHARED)
    install(TARGETS ito_shared
        EXPORT itoTargets
        ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
        LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
        RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
        INCLUDES DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}
    )
endif()
install(DIRECTORY include/ DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})

write_basic_package_version_file(
    ${CMAKE_CURRENT_BINARY_DIR}/itoConfigVersion.cmake
    COMPATIBILITY SameMajorVersion
)
configure_package_config_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/cmake/itoConfig.cmake.in
    ${CMAKE_CURRENT_BINARY_DIR}/itoConfig.cmake
    INSTALL_DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/ito
)
install(EXPORT itoTargets
    FILE itoTargets.cmake
    NAMESPACE ito::
    DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/ito
)
install(FILES
    ${CMAKE_CURRENT_BINARY_DIR}/itoConfig.cmake
    ${CMAKE_CURRENT_BINARY_DIR}/itoConfigVersion.cmake
    DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/ito
)
