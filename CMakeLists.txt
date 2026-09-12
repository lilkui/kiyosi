cmake_minimum_required(VERSION 3.28)

project(kiyosi VERSION 0.1.0 LANGUAGES CXX)

include(GenerateExportHeader)

if(PROJECT_IS_TOP_LEVEL)
    include(CTest)
    option(BUILD_SHARED_LIBS "Build shared libraries" OFF)
endif()
option(KIYOSI_INSTALL "Enable install rules" ${PROJECT_IS_TOP_LEVEL})
option(KIYOSI_ENABLE_CLANG_TIDY "Run clang-tidy while compiling project targets" OFF)
option(KIYOSI_ENABLE_CLANG_FORMAT "Add clang-format and clang-format-check targets" OFF)
option(KIYOSI_ENABLE_SANITIZERS "Enable AddressSanitizer and UndefinedBehaviorSanitizer" OFF)
option(KIYOSI_BUILD_PYTHON "Build the Python bindings" OFF)

add_library(kiyosi_warnings INTERFACE)
add_library(kiyosi::warnings ALIAS kiyosi_warnings)
if(CMAKE_CXX_COMPILER_FRONTEND_VARIANT STREQUAL "MSVC")
    target_compile_options(kiyosi_warnings INTERFACE /W4 /permissive-)
else()
    target_compile_options(kiyosi_warnings INTERFACE
        "$<$<COMPILE_LANG_AND_ID:CXX,GNU,Clang,AppleClang,Intel,IntelLLVM>:-Wall;-Wextra;-Wpedantic>"
    )
endif()

add_library(kiyosi_sanitizers INTERFACE)
add_library(kiyosi::sanitizers ALIAS kiyosi_sanitizers)
if(KIYOSI_ENABLE_SANITIZERS)
    if(NOT CMAKE_CXX_COMPILER_ID MATCHES "^(GNU|Clang|AppleClang)$"
       OR CMAKE_CXX_COMPILER_FRONTEND_VARIANT STREQUAL "MSVC")
        message(FATAL_ERROR "KIYOSI_ENABLE_SANITIZERS requires GCC or Clang with a GNU-style frontend")
    endif()
    target_compile_options(kiyosi_sanitizers INTERFACE
        "$<$<COMPILE_LANG_AND_ID:CXX,GNU,Clang,AppleClang>:-fsanitize=address,undefined;-fno-omit-frame-pointer>"
    )
    target_link_options(kiyosi_sanitizers INTERFACE
        "$<$<LINK_LANG_AND_ID:CXX,GNU,Clang,AppleClang>:-fsanitize=address,undefined>"
    )
endif()

if(KIYOSI_ENABLE_CLANG_TIDY)
    find_program(KIYOSI_CLANG_TIDY_EXECUTABLE NAMES clang-tidy REQUIRED)
endif()

if(KIYOSI_ENABLE_CLANG_FORMAT)
    find_program(KIYOSI_CLANG_FORMAT_EXECUTABLE NAMES clang-format REQUIRED)
    file(GLOB_RECURSE KIYOSI_CLANG_FORMAT_SOURCES
        "${CMAKE_CURRENT_SOURCE_DIR}/include/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/*.hpp"
    )
    add_custom_target(kiyosi-format
        COMMAND "${KIYOSI_CLANG_FORMAT_EXECUTABLE}" -i ${KIYOSI_CLANG_FORMAT_SOURCES}
        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
        VERBATIM
        USES_TERMINAL
    )
    add_custom_target(kiyosi-format-check
        COMMAND "${KIYOSI_CLANG_FORMAT_EXECUTABLE}" --dry-run --Werror ${KIYOSI_CLANG_FORMAT_SOURCES}
        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
        VERBATIM
        USES_TERMINAL
    )
endif()

add_library(kiyosi)
add_library(kiyosi::kiyosi ALIAS kiyosi)
generate_export_header(kiyosi
    EXPORT_FILE_NAME "${CMAKE_CURRENT_BINARY_DIR}/generated/kiyosi/kiyosi_export.h"
)
target_compile_features(kiyosi PUBLIC cxx_std_23)
if(NOT BUILD_SHARED_LIBS)
    target_compile_definitions(kiyosi PUBLIC KIYOSI_STATIC_DEFINE)
endif()
target_link_libraries(kiyosi PRIVATE $<BUILD_INTERFACE:kiyosi::warnings> $<BUILD_INTERFACE:kiyosi::sanitizers>)
target_sources(kiyosi
    PRIVATE
        src/core/types.cpp
        src/pricing/engines/asian/average.cpp
        src/pricing/engines/barrier/analytic.cpp
        src/pricing/engines/barrier/binary.cpp
        src/pricing/engines/barrier/finite_difference.cpp
        src/pricing/engines/digital/analytic.cpp
        src/pricing/engines/digital/finite_difference.cpp
        src/pricing/engines/digital/integral.cpp
        src/pricing/engines/structured/finite_difference.cpp
        src/pricing/engines/structured/monte_carlo.cpp
        src/pricing/engines/vanilla/analytic.cpp
        src/pricing/engines/vanilla/binomial.cpp
        src/pricing/engines/vanilla/bjerksund_stensland.cpp
        src/pricing/engines/vanilla/finite_difference.cpp
        src/pricing/engines/vanilla/integral.cpp
        src/pricing/engines/vanilla/monte_carlo.cpp
    PUBLIC
        FILE_SET HEADERS
        BASE_DIRS include
        FILES
            include/kiyosi/core/types.hpp
            include/kiyosi/core/schedule.hpp
            include/kiyosi/instruments/asian.hpp
            include/kiyosi/instruments/accumulator.hpp
            include/kiyosi/instruments/exercise_based_option.hpp
            include/kiyosi/instruments/option_terms.hpp
            include/kiyosi/instruments/barrier.hpp
            include/kiyosi/instruments/binary_barrier.hpp
            include/kiyosi/instruments/digital.hpp
            include/kiyosi/instruments/structured.hpp
            include/kiyosi/instruments/vanilla.hpp
            include/kiyosi/kiyosi.hpp
            include/kiyosi/market/calendar.hpp
            include/kiyosi/market/observation_schedule.hpp
            include/kiyosi/market/detail/sse_holidays.hpp
            include/kiyosi/market/context.hpp
            include/kiyosi/pricing/analytics.hpp
            include/kiyosi/pricing/engines/asian/average.hpp
            include/kiyosi/pricing/engines/barrier/analytic.hpp
            include/kiyosi/pricing/engines/barrier/binary.hpp
            include/kiyosi/pricing/engines/barrier/finite_difference.hpp
            include/kiyosi/pricing/engines/digital/analytic.hpp
            include/kiyosi/pricing/engines/digital/finite_difference.hpp
            include/kiyosi/pricing/engines/digital/integral.hpp
            include/kiyosi/pricing/engines/settings/finite_difference.hpp
            include/kiyosi/pricing/engines/settings/implied_volatility.hpp
            include/kiyosi/pricing/engines/structured/finite_difference.hpp
            include/kiyosi/pricing/engines/structured/monte_carlo.hpp
            include/kiyosi/pricing/engines/vanilla/analytic.hpp
            include/kiyosi/pricing/engines/vanilla/binomial.hpp
            include/kiyosi/pricing/engines/vanilla/bjerksund_stensland.hpp
            include/kiyosi/pricing/engines/vanilla/finite_difference.hpp
            include/kiyosi/pricing/engines/vanilla/integral.hpp
            include/kiyosi/pricing/engines/vanilla/monte_carlo.hpp
            include/kiyosi/pricing/result.hpp
        FILE_SET generated_headers TYPE HEADERS
        BASE_DIRS "${CMAKE_CURRENT_BINARY_DIR}/generated"
        FILES "${CMAKE_CURRENT_BINARY_DIR}/generated/kiyosi/kiyosi_export.h"
)
set_target_properties(kiyosi PROPERTIES
    CXX_EXTENSIONS OFF
    CXX_SCAN_FOR_MODULES OFF
    DEBUG_POSTFIX d
    POSITION_INDEPENDENT_CODE ON
    # During 0.x, minor versions define ABI compatibility; switch to major-only at 1.0.
    SOVERSION ${PROJECT_VERSION_MAJOR}.${PROJECT_VERSION_MINOR}
    VERSION ${PROJECT_VERSION}
    CXX_VISIBILITY_PRESET hidden
    VISIBILITY_INLINES_HIDDEN YES
)
if(PROJECT_IS_TOP_LEVEL AND BUILD_TESTING)
    include(FetchContent)
    FetchContent_Declare(
        Catch2
        GIT_REPOSITORY https://github.com/catchorg/Catch2.git
        GIT_TAG v3.16.0
        SYSTEM
        EXCLUDE_FROM_ALL
        FIND_PACKAGE_ARGS 3 CONFIG
    )
    FetchContent_MakeAvailable(Catch2)

    add_subdirectory(tests)
endif()

if(KIYOSI_ENABLE_CLANG_TIDY)
    set(KIYOSI_TIDY_TARGETS kiyosi)
    if(TARGET kiyosi_tests)
        list(APPEND KIYOSI_TIDY_TARGETS kiyosi_tests)
    endif()
    foreach(target_name IN LISTS KIYOSI_TIDY_TARGETS)
        set_target_properties(${target_name} PROPERTIES CXX_CLANG_TIDY "${KIYOSI_CLANG_TIDY_EXECUTABLE}")
    endforeach()
endif()

if(KIYOSI_BUILD_PYTHON)
    find_package(Python COMPONENTS Interpreter Development.Module REQUIRED)
    find_package(nanobind CONFIG REQUIRED)

    nanobind_add_module(_native python/kiyosi/_native.cpp)
    target_compile_features(_native PRIVATE cxx_std_23)
    target_link_libraries(_native PRIVATE kiyosi::kiyosi)
    install(TARGETS _native
        LIBRARY DESTINATION kiyosi
        RUNTIME DESTINATION kiyosi
    )
endif()

if(KIYOSI_INSTALL)
    include(CMakePackageConfigHelpers)
    include(GNUInstallDirs)
    install(TARGETS kiyosi
        EXPORT kiyosiTargets
        ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}" COMPONENT Development
        LIBRARY DESTINATION "${CMAKE_INSTALL_LIBDIR}" COMPONENT Runtime
            NAMELINK_COMPONENT Development
        RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}" COMPONENT Runtime
        FILE_SET HEADERS DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}" COMPONENT Development
        FILE_SET generated_headers DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}" COMPONENT Development
    )
    install(FILES LICENSE.txt
        DESTINATION "${CMAKE_INSTALL_DATADIR}/licenses/kiyosi"
        COMPONENT Runtime
    )
    write_basic_package_version_file(
        "${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfigVersion.cmake"
        # Before 1.0, minor versions define compatibility rather than major versions.
        COMPATIBILITY SameMinorVersion
    )
    configure_package_config_file(
        "${CMAKE_CURRENT_SOURCE_DIR}/cmake/kiyosiConfig.cmake.in"
        "${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfig.cmake"
        INSTALL_DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/kiyosi"
    )
    install(EXPORT kiyosiTargets
        FILE kiyosiTargets.cmake
        NAMESPACE kiyosi::
        DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/kiyosi"
        COMPONENT Development
    )
    install(FILES
        "${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfig.cmake"
        "${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfigVersion.cmake"
        DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/kiyosi"
        COMPONENT Development
    )
endif()
