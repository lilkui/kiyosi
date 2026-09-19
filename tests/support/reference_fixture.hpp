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

using FixtureAttributes = std::map<std::string, std::string>;

struct MonteCarloMetadata {
    std::uint64_t seed = 0;
    std::size_t paths = 0;
    std::size_t step_count = 0;
    double tolerance = 0.0;
};

struct ReferenceProvenance {
    std::string source_revision;
    std::string source_symbol;
    std::string convention;
    std::string reference_kind;
    double explicit_tolerance = 0.0;
};

struct ReferenceCase {
    std::string case_id;
    std::string instrument;
    std::string engine;
    std::string variant;
    FixtureAttributes inputs;
    ReferenceProvenance provenance;
    std::map<std::string, double> outputs;
    std::map<std::string, double> tolerances;
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

inline Date calendar_date(const std::vector<std::string>& fields, std::size_t& index,
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
            throw std::invalid_argument("Date component");
        }
        const Date value{std::chrono::year{year} / std::chrono::month{month} /
                         std::chrono::day{day_number}};
        if (!is_valid_date(value)) throw std::invalid_argument("Date");
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

} // namespace detail

inline std::vector<ReferenceCase> parse_reference_cases(std::istream& input, char delimiter = '\0')
{
    using namespace detail;
    constexpr std::array columns{"case_id", "instrument", "engine", "variant", "inputs", "outputs",
                                 "tolerances", "monte_carlo"};
    std::string line;
    std::size_t row = 0;
    std::vector<ReferenceCase> cases;
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
        ReferenceCase value;
        value.case_id = field(fields, 0, row, "case_id");
        value.instrument = field(fields, 1, row, "instrument");
        value.engine = field(fields, 2, row, "engine");
        value.variant = field(fields, 3, row, "variant");
        value.inputs = attributes(fields[4], row, "inputs");
        const auto required_input = [&](std::string_view name) {
            const auto found = value.inputs.find(std::string{name});
            if (found == value.inputs.end() || found->second.empty())
                throw FixtureParseError("fixture row " + std::to_string(row) + ": missing provenance " + std::string{name});
            return found->second;
        };
        value.provenance = ReferenceProvenance{
            required_input("source_revision"), required_input("source_symbol"),
            required_input("convention"), required_input("reference_kind"), 0.0};
        if (value.provenance.reference_kind != "analytic" &&
            value.provenance.reference_kind != "approximate" &&
            value.provenance.reference_kind != "discretized" &&
            value.provenance.reference_kind != "statistical")
            throw FixtureParseError("fixture row " + std::to_string(row) +
                                    ": reference_kind must be analytic, approximate, discretized, or statistical");
        const auto tolerance_text = required_input("tolerance");
        try {
            std::size_t parsed = 0;
            value.provenance.explicit_tolerance = std::stod(tolerance_text, &parsed);
            if (parsed != tolerance_text.size() || !std::isfinite(value.provenance.explicit_tolerance) ||
                value.provenance.explicit_tolerance < 0.0)
                throw std::invalid_argument("tolerance");
        } catch (const std::exception&) {
            throw FixtureParseError("fixture row " + std::to_string(row) +
                                    ": provenance tolerance must be finite and non-negative");
        }
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
        if (value.inputs.contains("owner") && value.inputs.at("owner") == "QuantLib") {
            const auto invalid = [&] {
                return FixtureParseError("fixture row " + std::to_string(row) + ": invalid Greek declaration");
            };
            const std::map<std::string, std::string> units{
                {"price", "price"}, {"delta", "price/spot"}, {"gamma", "price/spot^2"},
                {"speed", "price/spot^3"}, {"theta", "price/day"}, {"charm", "delta/day"},
                {"color", "gamma/day"}, {"vega", "price/volatility-pp"},
                {"vanna", "delta/volatility-pp"}, {"zomma", "gamma/volatility-pp"}, {"rho", "price/rate-pp"}};
            std::size_t index = 0;
            const auto expiry_date = calendar_date({required_input("expiry_date")}, index, row, "expiry_date");
            index = 0;
            const auto valuation = calendar_date({required_input("valuation")}, index, row, "valuation");
            const bool expiry_boundary = (expiry_date - valuation).count() <= 2;
            index = 0;
            const bool exercise_boundary = value.instrument == "AmericanOption" &&
                (valuation - calendar_date({required_input("effective_date")}, index, row, "effective_date")).count() < 2;
            const bool boundary = expiry_boundary || exercise_boundary;
            const bool binary_product = value.instrument == "BinaryBarrierOption" ||
                                        value.instrument == "TouchOption";
            const bool binary_expiry = binary_product && expiry_date == valuation;
            const bool asian = value.instrument == "GeometricAveragePriceOption" || value.instrument == "ArithmeticAveragePriceOption";
            const bool asian_expiry = asian && expiry_date == valuation;
            index = 0;
            const bool asian_start = asian &&
                (valuation - calendar_date({required_input("averaging_start_date")}, index, row, "averaging_start_date")).count() <= 2;
            index = 0;
            const bool binary_boundary = binary_product &&
                number({required_input("spot")}, index, row, "spot") == [&] {
                    std::size_t position = 0;
                    return number({required_input("barrier")}, position, row, "barrier");
                }();
            std::size_t available = 0;
            for (const auto& [name, unit] : units) {
                const bool unavailable = ((binary_boundary || binary_expiry || asian_expiry || asian_start) && name != "price") ||
                    (boundary && (name == "theta" || name == "charm" || name == "color"));
                if (required_input("unit_" + name) != unit ||
                    value.inputs.contains("unavailable_" + name) != unavailable ||
                    value.outputs.contains(name) == unavailable) throw invalid();
                if (unavailable) {
                    if (required_input("unavailable_" + name) != (asian_expiry ?
                        "terminal average payoff: no smooth sensitivities" : asian_start ?
                        "averaging-start boundary: price only, no smooth time stencil" : binary_expiry ?
                        "terminal payoff: no smooth sensitivities" : binary_boundary ?
                        "spot equals barrier: hit-state boundary" : expiry_boundary ?
                        "whole-day stability stencil touches expiry_date" : "whole-day stability stencil precedes exercise window"))
                        throw invalid();
                    continue;
                }
                ++available;
                const auto read = [&](const std::string& prefix) {
                    std::size_t position = 0;
                    const double result = number({required_input(prefix + name)}, position, row, prefix + name);
                    check_tolerance(result, row, prefix + name);
                    return result;
                };
                if (read("uncertainty_") > read("stability_limit_")) throw invalid();
                (void)read("numerical_tolerance_");
            }
            if (value.outputs.size() != available) throw invalid();
            for (const auto& [name, reason] : value.inputs) {
                if (name.starts_with("unavailable_") && !units.contains(name.substr(12))) throw invalid();
            }
        }
        if (!fields[7].empty() && fields[7] != "-") {
            const auto parts = split(fields[7], '|');
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

inline std::vector<ReferenceCase> load_reference_cases(const std::filesystem::path& path)
{
    std::ifstream input(path);
    if (!input) throw FixtureParseError("cannot open fixture '" + path.string() + "'");
    return parse_reference_cases(input);
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
    const ReferenceCase& fixture, const std::map<std::string, double>& actual)
{
    std::vector<FixtureFailure> failures;
    for (const auto& [name, expected] : fixture.outputs) {
        const auto value = actual.at(name);
        const auto tolerance = fixture.tolerances.at(name);
        if (!within_tolerance(value, expected, tolerance)) {
            failures.push_back(FixtureFailure{fixture.case_id, name, expected, value, tolerance});
        }
    }
    return failures;
}

inline void check_fixture(const ReferenceCase& fixture, const std::map<std::string, double>& actual)
{
    for (const auto& failure : compare_fixture(fixture, actual)) {
        INFO(failure.message());
        CHECK_THAT(failure.actual, Catch::Matchers::WithinAbs(failure.expected, failure.tolerance));
    }
}

} // namespace kiyosi::test
