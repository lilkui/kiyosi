cmake_minimum_required(VERSION 3.25)

project(ito VERSION 0.1.0 LANGUAGES CXX)

include(CMakePackageConfigHelpers)
include(CTest)
include(GNUInstallDirs)

option(ITO_BUILD_SHARED "Build the optional shared library" OFF)
option(ITO_ENABLE_CLANG_TIDY "Run clang-tidy while compiling project targets" OFF)
option(ITO_ENABLE_CLANG_FORMAT "Add clang-format and clang-format-check targets" OFF)

if(ITO_ENABLE_CLANG_TIDY)
    find_program(ITO_CLANG_TIDY_EXECUTABLE NAMES clang-tidy)
    if(NOT ITO_CLANG_TIDY_EXECUTABLE OR NOT EXISTS "${ITO_CLANG_TIDY_EXECUTABLE}")
        message(FATAL_ERROR "ITO_ENABLE_CLANG_TIDY requires clang-tidy")
    endif()
endif()

if(ITO_ENABLE_CLANG_FORMAT)
    find_program(ITO_CLANG_FORMAT_EXECUTABLE NAMES clang-format)
    if(NOT ITO_CLANG_FORMAT_EXECUTABLE OR NOT EXISTS "${ITO_CLANG_FORMAT_EXECUTABLE}")
        message(FATAL_ERROR "ITO_ENABLE_CLANG_FORMAT requires clang-format")
    endif()
    file(GLOB_RECURSE ITO_CLANG_FORMAT_SOURCES CONFIGURE_DEPENDS
        ${CMAKE_CURRENT_SOURCE_DIR}/include/*.hpp
        ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp
        ${CMAKE_CURRENT_SOURCE_DIR}/tests/*.cpp
        ${CMAKE_CURRENT_SOURCE_DIR}/tests/*.hpp
    )
    add_custom_target(format
        COMMAND ${ITO_CLANG_FORMAT_EXECUTABLE} -i ${ITO_CLANG_FORMAT_SOURCES}
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        VERBATIM
        USES_TERMINAL
    )
    add_custom_target(format-check
        COMMAND ${ITO_CLANG_FORMAT_EXECUTABLE} --dry-run --Werror ${ITO_CLANG_FORMAT_SOURCES}
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        VERBATIM
        USES_TERMINAL
    )
endif()

set(ITO_SOURCES
    src/core/types.cpp
    src/pricing/engines/analytic.cpp
    src/pricing/engines/binomial.cpp
    src/pricing/engines/finite_difference.cpp
    src/pricing/engines/digital_barrier.cpp
)

add_library(ito STATIC ${ITO_SOURCES})
add_library(ito::ito ALIAS ito)
target_compile_features(ito PUBLIC cxx_std_23)
target_include_directories(ito
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
)
set_target_properties(ito PROPERTIES EXPORT_NAME ito)
if(ITO_ENABLE_CLANG_TIDY)
    set_target_properties(ito PROPERTIES CXX_CLANG_TIDY "${ITO_CLANG_TIDY_EXECUTABLE}")
endif()

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
    if(ITO_ENABLE_CLANG_TIDY)
        set_target_properties(ito_shared PROPERTIES CXX_CLANG_TIDY "${ITO_CLANG_TIDY_EXECUTABLE}")
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

    add_executable(ito_tests tests/smoke_tests.cpp tests/parity_fixture_tests.cpp tests/header_smoke.cpp
                              tests/validation_tests.cpp)
    target_link_libraries(ito_tests PRIVATE ito Catch2::Catch2WithMain)
    target_compile_features(ito_tests PRIVATE cxx_std_23)
    target_compile_definitions(ito_tests PRIVATE ITO_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}")
    if(ITO_ENABLE_CLANG_TIDY)
        set_target_properties(ito_tests PROPERTIES CXX_CLANG_TIDY "${ITO_CLANG_TIDY_EXECUTABLE}")
    endif()

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
