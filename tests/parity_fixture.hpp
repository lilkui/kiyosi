#pragma once

#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>

namespace kiyosi::test {

struct ParityFixture {
    std::string case_id;
    option_type option;
    date valuation_date;
    date expiry;
    double spot;
    double strike;
    double risk_free_rate;
    double dividend_yield;
    double volatility;
    double observed_price;

    double value;
    double value_tolerance;
    double delta;
    double delta_tolerance;
    double gamma;
    double gamma_tolerance;
    double speed;
    double speed_tolerance;
    double theta;
    double theta_tolerance;
    double charm;
    double charm_tolerance;
    double color;
    double color_tolerance;
    double vega;
    double vega_tolerance;
    double vanna;
    double vanna_tolerance;
    double zomma;
    double zomma_tolerance;
    double rho;
    double rho_tolerance;
    double implied_volatility;
    double implied_volatility_tolerance;
};

using FixtureAttributes = std::map<std::string, std::string>;

struct ValidationExpectation {
    std::string category;
    std::string message;
};

struct ConvergenceMetadata {
    std::string parameter;
    std::vector<double> resolutions;
    double reference = 0.0;
    double tolerance = 0.0;
};

struct MonteCarloMetadata {
    std::uint64_t seed = 0;
    std::size_t paths = 0;
    std::size_t steps = 0;
    double tolerance = 0.0;
};

struct ParityCase {
    std::string case_id;
    std::string instrument;
    std::string engine;
    std::string variant;
    FixtureAttributes inputs;
    std::map<std::string, double> outputs;
    std::map<std::string, double> tolerances;
    std::optional<ValidationExpectation> validation;
    std::optional<ConvergenceMetadata> convergence;
    std::optional<MonteCarloMetadata> monte_carlo;
};

class FixtureParseError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

namespace detail {

inline std::vector<std::string> split(std::string_view line, char delimiter)
{
    std::vector<std::string> fields;
    std::size_t start = 0;
    while (start <= line.size()) {
        const auto end = line.find(delimiter, start);
        fields.emplace_back(line.substr(start, end == std::string_view::npos ? end : end - start));
        if (end == std::string_view::npos) break;
        start = end + 1;
    }
    return fields;
}

inline std::string field(const std::vector<std::string>& fields, std::size_t index,
                         std::size_t row, std::string_view name)
{
    if (fields[index].empty()) {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": empty " + std::string{name});
    }
    return fields[index];
}

inline double number(const std::vector<std::string>& fields, std::size_t& index,
                     std::size_t row, std::string_view name)
{
    const auto text = field(fields, index++, row, name);
    std::size_t parsed = 0;
    double value = 0.0;
    try {
        value = std::stod(text, &parsed);
    } catch (const std::exception&) {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                std::string{name} + " '" + text + "'");
    }
    if (parsed != text.size() || !std::isfinite(value)) {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                std::string{name} + " '" + text + "'");
    }
    return value;
}

inline date calendar_date(const std::vector<std::string>& fields, std::size_t& index,
                          std::size_t row, std::string_view name)
{
    const auto text = field(fields, index++, row, name);
    if (text.size() != 10 || text[4] != '-' || text[7] != '-') {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                std::string{name} + " '" + text + "' (expected YYYY-MM-DD)");
    }
    try {
        std::size_t year_length = 0;
        std::size_t month_length = 0;
        std::size_t day_length = 0;
        const auto year = std::stoi(text.substr(0, 4), &year_length);
        const auto month = static_cast<unsigned>(std::stoul(text.substr(5, 2), &month_length));
        const auto day_number = static_cast<unsigned>(std::stoul(text.substr(8, 2), &day_length));
        if (year_length != 4 || month_length != 2 || day_length != 2) {
            throw std::invalid_argument("date component");
        }
        const date value{std::chrono::year{year} / std::chrono::month{month} /
                         std::chrono::day{day_number}};
        if (!is_valid_date(value)) throw std::invalid_argument("date");
        return value;
    } catch (const std::exception&) {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                std::string{name} + " '" + text + "' (expected YYYY-MM-DD)");
    }
}

inline void check_tolerance(double tolerance, std::size_t row, std::string_view name)
{
    if (tolerance < 0.0) {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": " +
                                std::string{name} + " must be non-negative");
    }
}

inline FixtureAttributes attributes(std::string_view text, std::size_t row, std::string_view name)
{
    FixtureAttributes result;
    if (text.empty() || text == "-") return result;
    for (const auto& item : split(text, ';')) {
        const auto separator = item.find('=');
        if (separator == std::string::npos || separator == 0 || separator + 1 == item.size()) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                    std::string{name} + " entry '" + item + "'");
        }
        const auto key = item.substr(0, separator);
        if (result.contains(key)) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": duplicate " +
                                    std::string{name} + " key '" + key + "'");
        }
        result.emplace(key, item.substr(separator + 1));
    }
    return result;
}

inline std::map<std::string, double> numeric_attributes(std::string_view text, std::size_t row,
                                                         std::string_view name)
{
    std::map<std::string, double> result;
    for (const auto& [key, value] : attributes(text, row, name)) {
        std::size_t parsed = 0;
        double number = 0.0;
        try {
            number = std::stod(value, &parsed);
        } catch (const std::exception&) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                    std::string{name} + " value '" + value + "'");
        }
        if (parsed != value.size() || !std::isfinite(number)) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                    std::string{name} + " value '" + value + "'");
        }
        result.emplace(key, number);
    }
    return result;
}

inline std::vector<double> resolutions(std::string_view text, std::size_t row)
{
    std::vector<double> result;
    for (const auto& value : split(text, ',')) {
        std::size_t parsed = 0;
        double number = 0.0;
        try {
            number = std::stod(value, &parsed);
        } catch (const std::exception&) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid convergence resolution");
        }
        if (parsed != value.size() || !std::isfinite(number) || number <= 0.0)
            throw FixtureParseError("fixture row " + std::to_string(row) + ": convergence resolutions must be positive");
        result.push_back(number);
    }
    if (result.empty()) throw FixtureParseError("fixture row " + std::to_string(row) + ": convergence requires resolutions");
    return result;
}

} // namespace detail

inline std::vector<ParityCase> parse_parity_cases(std::istream& input, char delimiter = '\0')
{
    using namespace detail;
    constexpr std::array columns{"case_id", "instrument", "engine", "variant", "inputs", "outputs",
                                 "tolerances", "validation", "convergence", "monte_carlo"};
    std::string line;
    std::size_t row = 0;
    std::vector<ParityCase> cases;
    bool header_read = false;
    while (std::getline(input, line)) {
        ++row;
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty() || line.front() == '#') continue;
        if (!header_read) {
            if (delimiter == '\0') delimiter = line.find('\t') != std::string::npos ? '\t' : ',';
            const auto header = split(line, delimiter);
            if (header.size() != columns.size())
                throw FixtureParseError("fixture header: expected " + std::to_string(columns.size()) +
                                        " columns, got " + std::to_string(header.size()));
            for (std::size_t index = 0; index < columns.size(); ++index)
                if (header[index] != columns[index])
                    throw FixtureParseError("fixture header: column " + std::to_string(index + 1) +
                                            " must be '" + columns[index] + "'");
            header_read = true;
            continue;
        }
        const auto fields = split(line, delimiter);
        if (fields.size() != columns.size())
            throw FixtureParseError("fixture row " + std::to_string(row) + ": expected " +
                                    std::to_string(columns.size()) + " columns, got " +
                                    std::to_string(fields.size()));
        ParityCase value;
        value.case_id = field(fields, 0, row, "case_id");
        value.instrument = field(fields, 1, row, "instrument");
        value.engine = field(fields, 2, row, "engine");
        value.variant = field(fields, 3, row, "variant");
        value.inputs = attributes(fields[4], row, "inputs");
        value.outputs = numeric_attributes(fields[5], row, "outputs");
        value.tolerances = numeric_attributes(fields[6], row, "tolerances");
        if (value.outputs.size() != value.tolerances.size())
            throw FixtureParseError("fixture row " + std::to_string(row) +
                                    ": outputs and tolerances must have matching keys");
        for (const auto& output : value.outputs)
            if (!value.tolerances.contains(output.first))
                throw FixtureParseError("fixture row " + std::to_string(row) +
                                        ": outputs and tolerances must have matching keys");
        for (const auto& [name, tolerance] : value.tolerances) check_tolerance(tolerance, row, name + " tolerance");
        if (!fields[7].empty() && fields[7] != "-") {
            const auto parts = split(fields[7], '|');
            if (parts.size() != 2 || parts[0].empty() || parts[1].empty())
                throw FixtureParseError("fixture row " + std::to_string(row) + ": validation must be category|message");
            value.validation = ValidationExpectation{parts[0], parts[1]};
        }
        if (!fields[8].empty() && fields[8] != "-") {
            const auto parts = split(fields[8], '|');
            if (parts.size() != 4 || parts[0].empty())
                throw FixtureParseError("fixture row " + std::to_string(row) + ": convergence must be parameter|resolutions|reference|tolerance");
            try {
                std::size_t parsed = 0;
                const auto reference = std::stod(parts[2], &parsed);
                if (parsed != parts[2].size() || !std::isfinite(reference)) throw std::invalid_argument("reference");
                const auto tolerance = std::stod(parts[3], &parsed);
                if (parsed != parts[3].size() || !std::isfinite(tolerance) || tolerance < 0.0)
                    throw std::invalid_argument("tolerance");
                value.convergence = ConvergenceMetadata{parts[0], resolutions(parts[1], row), reference, tolerance};
            } catch (const std::exception&) {
                throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid convergence metadata");
            }
        }
        if (!fields[9].empty() && fields[9] != "-") {
            const auto parts = split(fields[9], '|');
            if (parts.size() != 4)
                throw FixtureParseError("fixture row " + std::to_string(row) + ": Monte Carlo must be seed|paths|steps|tolerance");
            std::size_t seed = 0, paths = 0, steps = 0, parsed = 0;
            try {
                seed = std::stoull(parts[0], &parsed); if (parsed != parts[0].size()) throw std::invalid_argument("seed");
                paths = std::stoull(parts[1], &parsed); if (parsed != parts[1].size()) throw std::invalid_argument("paths");
                steps = std::stoull(parts[2], &parsed); if (parsed != parts[2].size()) throw std::invalid_argument("steps");
                const auto tolerance = std::stod(parts[3], &parsed);
                if (paths == 0 || steps == 0 || parsed != parts[3].size() || !std::isfinite(tolerance) || tolerance < 0.0)
                    throw std::invalid_argument("tolerance");
                value.monte_carlo = MonteCarloMetadata{seed, paths, steps, tolerance};
            } catch (const std::exception&) {
                throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid Monte Carlo metadata");
            }
        }
        cases.push_back(std::move(value));
    }
    if (!header_read) throw FixtureParseError("fixture is missing a header");
    if (cases.empty()) throw FixtureParseError("fixture contains no data rows");
    return cases;
}

inline std::vector<ParityCase> load_parity_cases(const std::filesystem::path& path)
{
    std::ifstream input(path);
    if (!input) throw FixtureParseError("cannot open fixture '" + path.string() + "'");
    return parse_parity_cases(input);
}

inline std::vector<ParityFixture> parse_parity_fixtures(std::istream& input, char delimiter = '\0')
{
    constexpr std::array columns{
        "case_id", "option_type", "valuation_date", "expiry", "spot", "strike",
        "risk_free_rate", "dividend_yield", "volatility", "observed_price",
        "value", "value_tolerance", "delta", "delta_tolerance", "gamma", "gamma_tolerance",
        "speed", "speed_tolerance", "theta", "theta_tolerance", "charm", "charm_tolerance",
        "color", "color_tolerance", "vega", "vega_tolerance", "vanna", "vanna_tolerance",
        "zomma", "zomma_tolerance", "rho", "rho_tolerance", "implied_volatility",
        "implied_volatility_tolerance"};

    std::string line;
    std::size_t row = 0;
    std::vector<ParityFixture> fixtures;
    bool header_read = false;
    while (std::getline(input, line)) {
        ++row;
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty() || line.front() == '#') continue;
        if (!header_read) {
            if (delimiter == '\0') delimiter = line.find('\t') != std::string::npos ? '\t' : ',';
            const auto header = detail::split(line, delimiter);
            if (header.size() != columns.size()) {
                throw FixtureParseError("fixture header: expected " + std::to_string(columns.size()) +
                                        " columns, got " + std::to_string(header.size()));
            }
            for (std::size_t index = 0; index < columns.size(); ++index) {
                if (header[index] != columns[index]) {
                    throw FixtureParseError("fixture header: column " + std::to_string(index + 1) +
                                            " must be '" + columns[index] + "'");
                }
            }
            header_read = true;
            continue;
        }

        const auto fields = detail::split(line, delimiter);
        if (fields.size() != columns.size()) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": expected " +
                                    std::to_string(columns.size()) + " columns, got " +
                                    std::to_string(fields.size()));
        }
        std::size_t index = 0;
        ParityFixture fixture{};
        fixture.case_id = detail::field(fields, index++, row, "case_id");
        const auto option_text = detail::field(fields, index++, row, "option_type");
        if (option_text == "call") fixture.option = option_type::call;
        else if (option_text == "put") fixture.option = option_type::put;
        else throw FixtureParseError("fixture row " + std::to_string(row) + ": option_type must be call or put");
        fixture.valuation_date = detail::calendar_date(fields, index, row, "valuation_date");
        fixture.expiry = detail::calendar_date(fields, index, row, "expiry");
        fixture.spot = detail::number(fields, index, row, "spot");
        fixture.strike = detail::number(fields, index, row, "strike");
        fixture.risk_free_rate = detail::number(fields, index, row, "risk_free_rate");
        fixture.dividend_yield = detail::number(fields, index, row, "dividend_yield");
        fixture.volatility = detail::number(fields, index, row, "volatility");
        fixture.observed_price = detail::number(fields, index, row, "observed_price");
#define KIYOSI_FIXTURE_OUTPUT(name)                                                    \
    fixture.name = detail::number(fields, index, row, #name);                          \
    fixture.name##_tolerance = detail::number(fields, index, row, #name "_tolerance"); \
    detail::check_tolerance(fixture.name##_tolerance, row, #name "_tolerance");
        KIYOSI_FIXTURE_OUTPUT(value)
        KIYOSI_FIXTURE_OUTPUT(delta)
        KIYOSI_FIXTURE_OUTPUT(gamma)
        KIYOSI_FIXTURE_OUTPUT(speed)
        KIYOSI_FIXTURE_OUTPUT(theta)
        KIYOSI_FIXTURE_OUTPUT(charm)
        KIYOSI_FIXTURE_OUTPUT(color)
        KIYOSI_FIXTURE_OUTPUT(vega)
        KIYOSI_FIXTURE_OUTPUT(vanna)
        KIYOSI_FIXTURE_OUTPUT(zomma)
        KIYOSI_FIXTURE_OUTPUT(rho)
#undef KIYOSI_FIXTURE_OUTPUT
        fixture.implied_volatility = detail::number(fields, index, row, "implied_volatility");
        fixture.implied_volatility_tolerance =
            detail::number(fields, index, row, "implied_volatility_tolerance");
        detail::check_tolerance(fixture.implied_volatility_tolerance, row,
                                "implied_volatility_tolerance");
        fixtures.push_back(std::move(fixture));
    }
    if (!header_read) throw FixtureParseError("fixture is missing a header");
    if (fixtures.empty()) throw FixtureParseError("fixture contains no data rows");
    return fixtures;
}

inline std::vector<ParityFixture> load_parity_fixtures(const std::filesystem::path& path)
{
    std::ifstream input(path);
    if (!input) throw FixtureParseError("cannot open fixture '" + path.string() + "'");
    return parse_parity_fixtures(input);
}

struct FixtureFailure {
    std::string case_id;
    std::string output;
    double expected;
    double actual;
    double tolerance;

    std::string message() const
    {
        std::ostringstream text;
        text << "case='" << case_id << "' output='" << output << "' expected="
             << expected << " actual=" << actual << " tolerance=" << tolerance;
        return text.str();
    }
};

inline bool within_tolerance(double actual, double expected, double tolerance) noexcept
{
    return std::isfinite(actual) && std::isfinite(expected) && std::isfinite(tolerance) &&
           tolerance >= 0.0 && std::abs(actual - expected) <= tolerance;
}

inline std::vector<FixtureFailure> compare_fixture(
    const ParityFixture& fixture, const PricingResult& actual, double implied_volatility)
{
    struct ExpectedOutput {
        std::string_view name;
        double expected;
        double tolerance;
    };
    const std::array<ExpectedOutput, 12> expected{{{"value", fixture.value, fixture.value_tolerance},
                                                   {"delta", fixture.delta, fixture.delta_tolerance},
                                                   {"gamma", fixture.gamma, fixture.gamma_tolerance},
                                                   {"speed", fixture.speed, fixture.speed_tolerance},
                                                   {"theta", fixture.theta, fixture.theta_tolerance},
                                                   {"charm", fixture.charm, fixture.charm_tolerance},
                                                   {"color", fixture.color, fixture.color_tolerance},
                                                   {"vega", fixture.vega, fixture.vega_tolerance},
                                                   {"vanna", fixture.vanna, fixture.vanna_tolerance},
                                                   {"zomma", fixture.zomma, fixture.zomma_tolerance},
                                                   {"rho", fixture.rho, fixture.rho_tolerance},
                                                   {"implied_volatility", fixture.implied_volatility, fixture.implied_volatility_tolerance}}};
    const std::array<double, 12> values{{*actual.get(risk_measure::price), *actual.get(risk_measure::delta),
                                         *actual.get(risk_measure::gamma), *actual.get(risk_measure::speed),
                                         *actual.get(risk_measure::theta), *actual.get(risk_measure::charm),
                                         *actual.get(risk_measure::color), *actual.get(risk_measure::vega),
                                         *actual.get(risk_measure::vanna), *actual.get(risk_measure::zomma),
                                         *actual.get(risk_measure::rho), implied_volatility}};
    std::vector<FixtureFailure> failures;
    for (std::size_t index = 0; index < values.size(); ++index) {
        if (!within_tolerance(values[index], expected[index].expected, expected[index].tolerance)) {
            failures.push_back(FixtureFailure{fixture.case_id, std::string{expected[index].name},
                                              expected[index].expected, values[index], expected[index].tolerance});
        }
    }
    return failures;
}

inline void check_fixture(const ParityFixture& fixture, const PricingResult& actual,
                          double implied_volatility)
{
    for (const auto& failure : compare_fixture(fixture, actual, implied_volatility)) {
        INFO(failure.message());
        CHECK_THAT(failure.actual, Catch::Matchers::WithinAbs(failure.expected, failure.tolerance));
    }
}

} // namespace kiyosi::test
