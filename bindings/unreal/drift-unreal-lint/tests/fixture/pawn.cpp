// Minimal synthetic stand-in for a UE Actor's Tick chain — not real UE
// headers, just enough shape (a class with a Tick-named method calling
// into a Simulate helper) to exercise reachability scoping without a full
// engine dependency for this tool's own tests.

// Real UE's FMath, trimmed to the one static method this fixture needs.
struct FMath
{
    static float Rand();
};

class ATestPawn
{
public:
    void Tick(float DeltaTime);
    void Simulate(float DeltaTime);
    void NotReached(float DeltaTime);
    float RollRandom();
    double ReadClock();

    float Velocity = 0.0f;
    float Position = 0.0f;
};

void ATestPawn::Tick(float DeltaTime)
{
    Simulate(DeltaTime);
}

// Reachable from Tick via Simulate: a real a+b+c float chain should fire
// exactly once (the whole chain, not once per operator).
void ATestPawn::Simulate(float DeltaTime)
{
    Velocity = Velocity + DeltaTime * 9.8f;
    Position = Position + Velocity + DeltaTime;
}

// Never called from Tick — must not be flagged even though it has the
// same float-chain shape as Simulate.
void ATestPawn::NotReached(float DeltaTime)
{
    Velocity = Velocity + DeltaTime * 9.8f;
}

// unseeded_rng fires here unconditionally, with or without any config —
// unlike float_outside_fixed_step, it doesn't need reachability scoping.
float ATestPawn::RollRandom()
{
    return FMath::Rand();
}

// Real UE's FPlatformTime, trimmed to the one static method this fixture
// needs — see main.rs's scan_wallclock_read comment for why the call is
// matched by call-site spelling rather than resolved declaration.
struct FPlatformTime
{
    static double Seconds();
};

// wallclock_read fires here unconditionally, same as unseeded_rng — no
// reachability scoping needed.
double ATestPawn::ReadClock()
{
    return FPlatformTime::Seconds();
}
