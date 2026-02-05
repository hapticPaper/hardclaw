# AI Safety Review Test Cases

## Safe Code Patterns

### 1. While True with Break (SAFE)
```python
def verify():
    while True:
        if condition_met():
            break
    return result
```
**Why safe**: Has explicit break condition, will terminate

### 2. Bounded Loop (SAFE)
```python
def verify():
    for i in range(100):
        process(i)
    return True
```
**Why safe**: Fixed iteration count, will terminate

### 3. While with Condition (SAFE)
```python
def verify():
    count = 0
    while count < 10:
        count += 1
    return True
```
**Why safe**: Clear termination condition

---

## Unsafe Code Patterns

### 1. While True Without Break (UNSAFE)
```python
def verify():
    while True:
        x = 1 + 1  # No break, no return
```
**Why unsafe**: Infinite loop, never terminates

### 2. While True with Unreachable Break (UNSAFE)
```python
def verify():
    while True:
        continue  # break never reached
        break
```
**Why unsafe**: Break is unreachable

### 3. Exponential Complexity (UNSAFE)
```python
def verify():
    for i in range(10000000):
        for j in range(10000000):
            x = i * j  # 10^14 operations
```
**Why unsafe**: Will timeout, excessive computation

### 4. Unbounded Recursion (UNSAFE)
```python
def verify(n):
    return verify(n + 1)  # No base case
```
**Why unsafe**: Stack overflow, never terminates

---

## Important Notes

1. **Real AI Models**: Production validators use GPT-4/Claude/etc which do proper control flow analysis
2. **Heuristic Limitations**: The demo heuristic is simplistic - just checks for `while True` + no `break`
3. **Sandbox Layer**: Runtime sandboxes catch exploits (network, file I/O), AI catches non-termination
4. **False Positives**: Minimized to avoid network disputes - when uncertain, mark SAFE
