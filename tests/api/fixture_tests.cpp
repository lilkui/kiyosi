#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers.hpp>

#include <array>
#include <chrono>
#include <string>
#include <utility>

#include "support/parity_fixture.hpp"

TEST_CASE("Parity fixture dates reject invalid calendar components", "[audit-fixes]")
{
    for (const std::string text : {"2025-02-30", "2025-02-29", "1900-02-29", "2025-04-31",
                                   "2025-13-01", "2025-00-01", "2025-01-00", "2025-01-32"}) {
        CAPTURE(text);
        CHECK_THROWS_WITH(kiyosi::test::detail::calendar_date(text, 7, "valuation"),
                          "fixture row 7: invalid valuation '" + text + "' (expected YYYY-MM-DD)");
    }
    using namespace std::chrono;
    for (const auto& [text, expected] : std::array{
             std::pair{"2024-02-29", year{2024} / February / 29},
             std::pair{"2000-02-29", year{2000} / February / 29},
             std::pair{"2025-01-01", year{2025} / January / 1},
             std::pair{"2025-12-31", year{2025} / December / 31}}) {
        CAPTURE(text);
        CHECK(kiyosi::test::detail::calendar_date(text, 7, "valuation") == kiyosi::Date{expected});
    }
}

TEST_CASE("Parity fixture dates retain diagnostics for malformed representations")
{
    for (const std::string text : {"", "2025-0x-06", "2025-01-0x", "202x-01-06", "2025-1-06", "2025-01-06x"}) {
        CAPTURE(text);
        CHECK_THROWS_AS(kiyosi::test::detail::calendar_date(text, 7, "valuation"),
                        kiyosi::test::FixtureParseError);
    }
}

TEST_CASE("Parity fixture attributes reject malformed and duplicate keys")
{
    using kiyosi::test::detail::attributes;
    CHECK(attributes("", 7, "inputs").empty());
    CHECK(attributes("-", 7, "inputs").empty());
    const kiyosi::test::FixtureAttributes expected{{"type", "call"}, {"strike", "100"}};
    CHECK(attributes("type=call;strike=100", 7, "inputs") == expected);
    for (const std::string text : {"type", "=call", "type=", "type=call;", "type=call;type=put"}) {
        CAPTURE(text);
        CHECK_THROWS_AS(attributes(text, 7, "inputs"), kiyosi::test::FixtureParseError);
    }
}
