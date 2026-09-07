cmake_minimum_required(VERSION 3.28)

project(kiyosi VERSION 0.1.0 LANGUAGES CXX)

include(CMakePackageConfigHelpers)
include(GenerateExportHeader)

if(PROJECT_IS_TOP_LEVEL)
    include(CTest)
endif()

if(PROJECT_IS_TOP_LEVEL)
    option(BUILD_SHARED_LIBS "Build shared libraries" OFF)
endif()
option(KIYOSI_INSTALL "Enable install rules" ${PROJECT_IS_TOP_LEVEL})
option(KIYOSI_ENABLE_CLANG_TIDY "Run clang-tidy while compiling project targets" OFF)
option(KIYOSI_ENABLE_CLANG_FORMAT "Add clang-format and clang-format-check targets" OFF)

add_library(kiyosi_warnings INTERFACE)
add_library(kiyosi::warnings ALIAS kiyosi_warnings)
target_compile_options(kiyosi_warnings INTERFACE
    "$<$<COMPILE_LANG_AND_ID:CXX,MSVC>:/W4;/permissive->"
    "$<$<COMPILE_LANG_AND_ID:CXX,GNU,Clang,AppleClang,Intel,IntelLLVM>:-Wall;-Wextra;-Wpedantic>"
)

if(KIYOSI_ENABLE_CLANG_TIDY)
    find_program(KIYOSI_CLANG_TIDY_EXECUTABLE NAMES clang-tidy REQUIRED)
endif()

if(KIYOSI_ENABLE_CLANG_FORMAT)
    find_program(KIYOSI_CLANG_FORMAT_EXECUTABLE NAMES clang-format REQUIRED)
    file(GLOB_RECURSE KIYOSI_CLANG_FORMAT_SOURCES CONFIGURE_DEPENDS
        ${CMAKE_CURRENT_SOURCE_DIR}/include/*.hpp
        ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp
        ${CMAKE_CURRENT_SOURCE_DIR}/tests/*.cpp
        ${CMAKE_CURRENT_SOURCE_DIR}/tests/*.hpp
    )
    add_custom_target(kiyosi-format
        COMMAND ${KIYOSI_CLANG_FORMAT_EXECUTABLE} -i ${KIYOSI_CLANG_FORMAT_SOURCES}
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        VERBATIM
        USES_TERMINAL
    )
    add_custom_target(kiyosi-format-check
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
    src/pricing/engines/monte_carlo.cpp
    src/pricing/engines/digital_barrier.cpp
    src/pricing/engines/asian.cpp
    src/pricing/engines/integral.cpp
    src/pricing/engines/digital_fd.cpp
    src/pricing/engines/binary_barrier.cpp
    src/pricing/engines/structured.cpp
    src/pricing/engines/barrier_fd.cpp
)

set(KIYOSI_HEADERS
    include/kiyosi/core/types.hpp
    include/kiyosi/instruments/asian.hpp
    include/kiyosi/instruments/barrier.hpp
    include/kiyosi/instruments/binary_barrier.hpp
    include/kiyosi/instruments/digital.hpp
    include/kiyosi/instruments/structured.hpp
    include/kiyosi/instruments/vanilla.hpp
    include/kiyosi/kiyosi.hpp
    include/kiyosi/market/calendar.hpp
    include/kiyosi/market/detail/sse_holidays.hpp
    include/kiyosi/market/context.hpp
    include/kiyosi/pricing/analytics.hpp
    include/kiyosi/pricing/engines/analytic.hpp
    include/kiyosi/pricing/engines/asian.hpp
    include/kiyosi/pricing/engines/barrier.hpp
    include/kiyosi/pricing/engines/binary_barrier.hpp
    include/kiyosi/pricing/engines/binomial.hpp
    include/kiyosi/pricing/engines/digital.hpp
    include/kiyosi/pricing/engines/digital_fd.hpp
    include/kiyosi/pricing/engines/finite_difference.hpp
    include/kiyosi/pricing/engines/integral.hpp
    include/kiyosi/pricing/engines/monte_carlo.hpp
    include/kiyosi/pricing/engines/structured.hpp
    include/kiyosi/pricing/result.hpp
)

add_library(kiyosi)
add_library(kiyosi::kiyosi ALIAS kiyosi)
generate_export_header(kiyosi
    EXPORT_FILE_NAME ${CMAKE_CURRENT_BINARY_DIR}/generated/kiyosi/kiyosi_export.h
)
target_include_directories(kiyosi PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<BUILD_INTERFACE:${CMAKE_CURRENT_BINARY_DIR}/generated>
)
target_compile_features(kiyosi PUBLIC cxx_std_23)
target_link_libraries(kiyosi PRIVATE kiyosi::warnings)
target_sources(kiyosi
    PRIVATE
        ${KIYOSI_SOURCES}
    PUBLIC
        FILE_SET HEADERS
        BASE_DIRS include
        FILES ${KIYOSI_HEADERS}
        FILE_SET generated_headers TYPE HEADERS
        BASE_DIRS ${CMAKE_CURRENT_BINARY_DIR}/generated
        FILES ${CMAKE_CURRENT_BINARY_DIR}/generated/kiyosi/kiyosi_export.h
)
set_target_properties(kiyosi PROPERTIES
    CXX_EXTENSIONS OFF
    DEBUG_POSTFIX d
    POSITION_INDEPENDENT_CODE ON
)
if(BUILD_SHARED_LIBS)
    set_target_properties(kiyosi PROPERTIES
        # During 0.x, minor versions define ABI compatibility; switch to major-only at 1.0.
        SOVERSION ${PROJECT_VERSION_MAJOR}.${PROJECT_VERSION_MINOR}
        VERSION ${PROJECT_VERSION}
        CXX_VISIBILITY_PRESET hidden
        VISIBILITY_INLINES_HIDDEN YES
    )
endif()
if(PROJECT_IS_TOP_LEVEL AND BUILD_TESTING)
    include(FetchContent)
    FetchContent_Declare(
        Catch2
        GIT_REPOSITORY https://github.com/catchorg/Catch2.git
        GIT_TAG 56809e5282f104c5c8b570e7c2996cdc352d94f1
        SYSTEM
        EXCLUDE_FROM_ALL
        FIND_PACKAGE_ARGS 3 CONFIG
    )
    FetchContent_MakeAvailable(Catch2)

    add_executable(kiyosi_tests tests/smoke_tests.cpp tests/parity_fixture_tests.cpp tests/header_smoke.cpp
                              tests/validation_tests.cpp tests/remaining_tests.cpp)
    target_link_libraries(kiyosi_tests PRIVATE kiyosi Catch2::Catch2WithMain kiyosi::warnings)
    set_target_properties(kiyosi_tests PROPERTIES CXX_EXTENSIONS OFF)
    target_compile_definitions(kiyosi_tests PRIVATE KIYOSI_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}")
    if(catch2_SOURCE_DIR)
        list(APPEND CMAKE_MODULE_PATH "${catch2_SOURCE_DIR}/extras")
    endif()
    include(Catch)
    catch_discover_tests(kiyosi_tests DISCOVERY_MODE PRE_TEST)
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

if(KIYOSI_INSTALL)
    include(GNUInstallDirs)
    target_include_directories(kiyosi PUBLIC
        $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
    )
    install(TARGETS kiyosi
        EXPORT kiyosiTargets
        ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR} COMPONENT Development
        LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR} COMPONENT Runtime
            NAMELINK_COMPONENT Development
        RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR} COMPONENT Runtime
        FILE_SET HEADERS DESTINATION ${CMAKE_INSTALL_INCLUDEDIR} COMPONENT Development
        FILE_SET generated_headers DESTINATION ${CMAKE_INSTALL_INCLUDEDIR} COMPONENT Development
    )
    install(FILES LICENSE.txt
        DESTINATION ${CMAKE_INSTALL_DATADIR}/licenses/kiyosi
        COMPONENT Runtime
    )
    write_basic_package_version_file(
        ${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfigVersion.cmake
        # Before 1.0, minor versions define compatibility rather than major versions.
        COMPATIBILITY SameMinorVersion
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
        COMPONENT Development
    )
    install(FILES
        ${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfig.cmake
        ${CMAKE_CURRENT_BINARY_DIR}/kiyosiConfigVersion.cmake
        DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/kiyosi
        COMPONENT Development
    )
endif()
