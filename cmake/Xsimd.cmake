include(FetchContent)

function(kiyosi_find_xsimd)
    find_package(xsimd 14.3.0 EXACT CONFIG QUIET)
    if(NOT xsimd_FOUND)
        set(XSIMD_SKIP_INSTALL ON)
        FetchContent_Declare(xsimd
            GIT_REPOSITORY https://github.com/xtensor-stack/xsimd.git
            GIT_TAG 14.3.0
            SYSTEM
            EXCLUDE_FROM_ALL
        )
        FetchContent_MakeAvailable(xsimd)
    endif()
endfunction()

kiyosi_find_xsimd()
