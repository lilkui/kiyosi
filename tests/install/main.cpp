#include <bit>
#include <chrono>
#include <cstdint>
#include <exception>
#include <kiyosi/kiyosi.hpp>

int main()
try {
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1};
    const Date end{std::chrono::year{2026} / 1 / 1};
    const auto option = make_european_option(OptionType::call, 100.0, start, end);
    const auto parameters = make_bsm_parameters(0.04, 0.01, 0.2);
    if (!option || !parameters) return 1;
    const auto context = make_pricing_context(*parameters, 100.0, start);
    if (!context) return 1;
    const auto serial = MonteCarloVanillaEngine{4097, 50, 42, MonteCarloBackend::cpu, 1}.price(*option, *context);
    const auto parallel = MonteCarloVanillaEngine{4097, 50, 42, MonteCarloBackend::cpu, 4}.price(*option, *context);
    if (!serial || !parallel) return 1;
    return std::bit_cast<std::uint64_t>(*serial) == std::bit_cast<std::uint64_t>(*parallel) && *serial > 0.0 ? 0 : 1;
} catch (const std::exception&) {
    return 1;
}
