#include <catch2/catch_test_macros.hpp>

#include <ito/ito.hpp>

TEST_CASE("ito exposes its version")
{
    REQUIRE(ito::version_major == 0);
    REQUIRE(ito::version_minor == 1);
    REQUIRE(ito::version_patch == 0);
}
