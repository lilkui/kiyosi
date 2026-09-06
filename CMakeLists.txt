cmake_minimum_required(VERSION 3.25)

project(kiyosi VERSION 0.1.0 LANGUAGES CXX)

include(CMakePackageConfigHelpers)
include(CTest)
include(GNUInstallDirs)

option(KIYOSI_BUILD_SHARED "Build the optional shared library" OFF)
option(KIYOSI_ENABLE_CLANG_TIDY "Run clang-tidy while compiling project targets" OFF)
option(KIYOSI_ENABLE_CLANG_FORMAT "Add clang-format and clang-format-check targets" OFF)

if(KIYOSI_ENABLE_CLANG_TIDY)
    find_program(KIYOSI_CLANG_TIDY_EXECUTABLE NAMES clang-tidy)
    if(NOT KIYOSI_CLANG_TIDY_EXECUTABLE OR NOT EXISTS "${KIYOSI_CLANG_TIDY_EXECUTABLE}")
        message(FATAL_ERROR "KIYOSI_ENABLE_CLANG_TIDY requires clang-tidy")
    endif()
endif()

if(KIYOSI_ENABLE_CLANG_FORMAT)
    find_program(KIYOSI_CLANG_FORMAT_EXECUTABLE NAMES clang-format)
    if(NOT KIYOSI_CLANG_FORMAT_EXECUTABLE OR NOT EXISTS "${KIYOSI_CLANG_FORMAT_EXECUTABLE}")
        message(FATAL_ERROR "KIYOSI_ENABLE_CLANG_FORMAT requires clang-format")
    endif()
    file(GLOB_RECURSE KIYOSI_CLANG_FORMAT_SOURCES CONFIGURE_DEPENDS
        ${CMAKE_CURRENT_SOURCE_DIR}/include/*.hpp
        ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp
        ${CMAKE_CURRENT_SOURCE_DIR}/tests/*.cpp
        ${CMAKE_CURRENT_SOURCE_DIR}/tests/*.hpp
    )
    add_custom_target(format
        COMMAND ${KIYOSI_CLANG_FORMAT_EXECUTABLE} -i ${KIYOSI_CLANG_FORMAT_SOURCES}
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        VERBATIM
        USES_TERMINAL
    )
    add_custom_target(format-check
        COMMAND ${KIYOSI_CLANG_FORMAT_EXECUTABLE} --dry-run --Werror ${KIYOSI_CLANG_FORMAT_SOURCES}
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        VERBATIM
        USES_TERMINAL
    )
endif()

set(KIYOSI_SOURCES
    src/core/types.cpp
    src/pricing/engines/analytic.cpp
    src/pricing/engines/binomial.cpp
    src/pricing/engines/finite_difference.cpp
    src/pricing/engines/digital_barrier.cpp
)

add_library(kiyosi STATIC ${KIYOSI_SOURCES})
add_library(kiyosi::kiyosi ALIAS kiyosi)
target_compile_features(kiyosi PUBLIC cxx_std_23)
target_include_directories(kiyosi
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
)
set_target_properties(kiyosi PROPERTIES EXPORT_NAME kiyosi)
if(KIYOSI_ENABLE_CLANG_TIDY)
    set_target_properties(kiyosi PROPERTIES CXX_CLANG_TIDY "${KIYOSI_CLANG_TIDY_EXECUTABLE}")
endif()

if(KIYOSI_BUILD_SHARED)
    add_library(kiyosi_shared SHARED ${KIYOSI_SOURCES})
    add_library(kiyosi::kiyosi_shared ALIAS kiyosi_shared)
    target_compile_features(kiyosi_shared PUBLIC cxx_std_23)
    target_include_directories(kiyosi_shared
        PUBLIC
            $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
            $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
    )
    set_target_properties(kiyosi_shared PROPERTIES EXPORT_NAME kiyosi_shared)
    if(KIYOSI_ENABLE_CLANG_TIDY)
        set_target_properties(kiyosi_shared PROPERTIES CXX_CLANG_TIDY "${KIYOSI_CLANG_TIDY_EXECUTABLE}")
    endif()
endif()

if(BUILD_TESTING)
    include(FetchContent)
    FetchContent_Declare(
        Catch2
        GIT_REPOSITORY https://github.com/catchorg/Catch2.git
        GIT_TAG 56809e5282f104c5c8b570e7c2996cdc352d94f1
    )
    FetchContent_MakeAvailable(Catch2)

    add_executable(kiyosi_tests tests/smoke_tests.cpp tests/parity_fixture_tests.cpp tests/header_smoke.cpp
                              tests/validation_tests.cpp)
    target_link_libraries(kiyosi_tests PRIVATE kiyosi Catch2::Catch2WithMain)
    target_compile_features(kiyosi_tests PRIVATE cxx_std_23)
    target_compile_definitions(kiyosi_tests PRIVATE KIYOSI_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}")
    if(KIYOSI_ENABLE_CLANG_TIDY)
        set_target_properties(kiyosi_tests PROPERTIES CXX_CLANG_TIDY "${KIYOSI_CLANG_TIDY_EXECUTABLE}")
    endif()

    list(APPEND CMAKE_MODULE_PATH ${catch2_SOURCE_DIR}/extras)
    include(Catch)
    catch_discover_tests(kiyosi_tests)
endif()

install(TARGETS kiyosi
    EXPORT kiyosiTargets
    ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
    INCLUDES DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}
)
if(KIYOSI_BUILD_SHARED)
    install(TARGETS kiyosi_shared
        EXPORT kiyosiTargets
        ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
        LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
        RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
        INCLUDES DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}
    )
endif()
install(DIRECTORY include/ DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})

write_basic_package_version_file(
    ${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfigVersion.cmake
    COMPATIBILITY SameMajorVersion
)
configure_package_config_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/cmake/kiyosiConfig.cmake.in
    ${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfig.cmake
    INSTALL_DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/kiyosi
)
install(EXPORT kiyosiTargets
    FILE kiyosiTargets.cmake
    NAMESPACE kiyosi::
    DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/kiyosi
)
install(FILES
    ${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfig.cmake
    ${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfigVersion.cmake
    DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/kiyosi
)
