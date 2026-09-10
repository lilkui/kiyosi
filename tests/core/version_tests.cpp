#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>

namespace {

TEST_CASE("kiyosi exposes its version")
{
    REQUIRE(kiyosi::version_major == 0);
    REQUIRE(kiyosi::version_minor == 1);
    REQUIRE(kiyosi::version_patch == 0);
}

}
