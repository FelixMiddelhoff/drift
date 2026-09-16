// Minimal synthetic stand-in for a UE Actor's Tick chain — not real UE
// headers, just enough shape (a class with a Tick-named method calling
// into a Simulate helper) to exercise reachability scoping without a full
// engine dependency for this tool's own tests.

// Real UE's FMath, trimmed to the one static method this fixture needs.
struct FMath
{
    static float Rand();
};

// Minimal synthetic stand-ins for TMap/TSet/TArray — just enough shape
// (begin()/end() for range-based-for, CreateIterator() for TMap) to
// exercise hashmap_iter's type-based detection without a full engine
// dependency.
template <typename K, typename V>
struct TMap
{
    struct Iterator
    {
        V* Ptr;
        V& operator*() const;
        void operator++();
        bool operator!=(const Iterator& Other) const;
    };
    Iterator begin() const;
    Iterator end() const;
    Iterator CreateIterator() const;
};

template <typename T>
struct TSet
{
    struct Iterator
    {
        T* Ptr;
        T& operator*() const;
        void operator++();
        bool operator!=(const Iterator& Other) const;
    };
    Iterator begin() const;
    Iterator end() const;
};

template <typename T>
struct TArray
{
    struct Iterator
    {
        T* Ptr;
        T& operator*() const;
        void operator++();
        bool operator!=(const Iterator& Other) const;
    };
    Iterator begin() const;
    Iterator end() const;
    void Sort();
};

class ATestPawn
{
public:
    void Tick(float DeltaTime);
    void Simulate(float DeltaTime);
    void NotReached(float DeltaTime);
    float RollRandom();
    double ReadClock();
    void IterateMap();
    void UseCreateIterator();
    void IterateSortedArray();
    void RunParallel();
    void LogAsync();

    float Velocity = 0.0f;
    float Position = 0.0f;
    TMap<int, float> ScoreByPlayer;
    TArray<int> SortedIds;
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

// hashmap_iter fires here unconditionally — range-based-for over a TMap.
void ATestPawn::IterateMap()
{
    for (auto& Pair : ScoreByPlayer)
    {
        (void)Pair;
    }
}

// hashmap_iter also fires on an explicit .CreateIterator() call, not just
// range-based-for.
void ATestPawn::UseCreateIterator()
{
    auto It = ScoreByPlayer.CreateIterator();
    (void)It;
}

// Must NOT fire: iterating a TArray (even one built from a map's values
// and sorted) is a different type than TMap/TSet — the type-based
// detection naturally excludes it, no separate collect-then-sort
// special-case needed the way the Rust rule required.
void ATestPawn::IterateSortedArray()
{
    SortedIds.Sort();
    for (auto& Id : SortedIds)
    {
        (void)Id;
    }
}

// Real UE's ParallelFor, trimmed to the one overload this fixture needs.
void ParallelFor(int Num, void (*Body)(int));

// Real UE's AsyncTask — deliberately NOT matched by
// unordered_parallelism, see main.rs's UNORDERED_PARALLELISM_FUNCS
// comment for the real evidence (checked against real non-Lyra Engine
// source, not assumed) behind that exclusion.
void AsyncTask(int ThreadType, void (*Body)());

// unordered_parallelism fires here unconditionally, same as
// unseeded_rng/wallclock_read/hashmap_iter — no reachability scoping
// needed.
void ATestPawn::RunParallel()
{
    ParallelFor(4, [](int) {});
}

// Must NOT fire: AsyncTask is deliberately excluded from v1 (see the
// comment on UNORDERED_PARALLELISM_FUNCS).
void ATestPawn::LogAsync()
{
    AsyncTask(0, []() {});
}

// Real UE's SIZE_T is itself a platform typedef — matched by spelling
// here too, same as FPlatformTime, not by resolving to its canonical
// underlying integer type.
using SIZE_T = unsigned long long;

struct FUnitId
{
    SIZE_T Handle;
    int DisplayIndex;
};

// usize_in_hashed_state fires here unconditionally — Handle (SIZE_T) is
// referenced inside a GetTypeHash overload for FUnitId, Unreal's
// free-function-found-via-ADL hashing convention (no derive/record
// equivalent to scan instead).
unsigned GetTypeHash(const FUnitId& Id)
{
    return static_cast<unsigned>(Id.Handle);
}

// Must NOT fire: RawHandle is SIZE_T, but never referenced inside any
// GetTypeHash overload — a pointer-width field alone isn't the hazard,
// only one actually read by a hash function is.
struct FUnusedHandle
{
    SIZE_T RawHandle;
};
