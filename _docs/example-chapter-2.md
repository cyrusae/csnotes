# Chapter 2: Java Fundamentals
### Breaking down a simple Java program:

```java
public class Simple {
	public static void main(String[] args) {
		System.out.println("Programming is great fun!");
	}
}
```

`public class Simple` = **class header**, begins a *class definition*
- **public** = *access specifier* key word
- **class** = key word indicating beginning of class definition
- `Simple` = class name

- Files can contain more than one class, but **only one `public` class** per Java file
- When a Java file has a `public` class, the. name of the `public` class must be the same as the name of the file
- Java is a case-sensitive language (including filenames!) 

`public static void main(String[] args)` = **method header** 
- "A method can be thought of as a group of one or more programming statements that collectively has a name."
- Every Java application must have a method named `main` 

- Brace characters are not statements, so they don't take a semicolon after them

### Simple variable declaration:

```java
public class Variable {
	public static void main(String[] args) {
		int value;
		value = 5;
		System.out.println("The value is ");
		System.out.println(value);
	}
}
```

- `int value;` = variable declaration (tells the compiler the name and data type of the variable)
- `value = 5;` = assignment statement (equals sign stores the right operand in the left operand)
- Can also be compressed: `int value = 5;`

- `+` can be used to concatenate strings (and will coerce numeric values to strings when used with strings)

- **identifier** = programmer-defined name that represents some part of a program
- Variable names and class names are both identifiers
- Identifiers cannot begin with a digit

### Primitive data types:

| Data type | Size       | Range                                                                                      |
| --------- | ---------- | ------------------------------------------------------------------------------------------ |
| `byte`    | 1 byte     | Integers `-128` to `127`                                                                   |
| `short`   | 2 bytes    | Integers `-32,768` to `32,767`                                                             |
| `int`     | 4 bytes    | Integers `-2,147,483,648` to `2,147,483,648`                                               |
| `long`    | 8 bytes    | Integers `-9,223,372,036,854,775,808` to `9,223,372,036,854,775,807`                       |
| `float`   | 4 bytes    | Floating-point numbers `+/-3.4 x 10^-38` to `+/-3.4 x 10^38`, with 7 digits of accuracy    |
| `double`  | 8 bytes    | Floating-point numbers `+/-1.7 x 10^-308` to `+/-1.7 x 10^308`, with 15 digits of accuracy |
| `char`    | 2(?) bytes | Single Unicode character, declared with single quotes (`char a = 'A';`)                    |
| `boolean` | 1 byte     | `true` or `false`                                                                          |
- "With the primitive data types, you can only create variables and a variable can only be used to hold a single value. Such variables do not have attributes or methods."
- Java will assume things that look like integers are integers; use `L` to force them into longs: `long example = 57L;`
- Java will assume floating-point values are `double`s; force floats with `F`: `float example = 78F`
- Java can use E notation for exponents: `2900000` = `2.9E6`

- Variables can be initialized in groups, and selectively: `int flightNum = 89, travelTime, departure = 10, distance;`
- When a variable is declared inside a method, it must have a value stored in it before it can be used

**Primitive type variables hold the literal data item with which they are associated.** 
#### Division
- Java performs *integer division* when both operands of division are integers, **even if the variable receiving the output isn't**
- `double number; number = 5 / 2;` will produce `number` with value `2`; the rest is truncated
- To force floating-point division you'd have to go `double number; number = 5.0 / 2;`

- Java API provides a `Math` class with methods like `.pow` (raise a number to a power) and `.sqrt` (return a square root)

### Combined assignment operators
Also called **compound operators**.

| Operator | Example    | Equivalent to |
| -------- | ---------- | ------------- |
| `+=`     | `x += 5;`  | `x = x + 5;`  |
| `-=`     | `y -= 2;`  | `y = y - 2;`  |
| `*=`     | `z *= 10;` | `z = z * 10;` |
| `/=`     | `a /= b;`  | `a = a / b;`  |
| `%=`     | `c %= 3;`  | `c = c % 3;`  |

### Type conversion
- Java will perform some type coercions but does not automatically perform coercions that could result in loss of data
- Allowed conversion: **widening conversion** 
- Disallowed conversion: *narrowing conversion*

- **Cast operator** syntax: `number = (int)78.3;` (will be 78)
- "When values of the `byte` or `short` data types are used in arithmetic expressions, they are temporarily converted to `int` values. The result of an arithmetic operation using only a mixture of `byte`, `short`, or `int` values will always be an `int`."

### Named constants
"Named constants are initialized with a value, and that value cannot change during the execution of the program." They use the keyword `final` and are customarily declared in `ALL_CAPS`: `final double INTEREST_RATE = 0.069;`

### `String` class and reference variables
- Java does not have a `String` primitive but does make a lot of special cases for strings
- The Java API provides a class for handling strings

- Strings are class type variables; "**A *class type variable* does not hold the actual data item that it is associated with, but ... the memory address of the data item it is associated with.**"
- Class type variables are known as *reference variables*

### Documentation comments
- Special kind of comment used to generate documentation (roxygen2-style)
- The first sentence in the method's documentation comment is used as the summary of the method (sentenced defined as "until first period followed by a whitespace")
- `javadoc SourceFile.java` will create documentation using documentation comments in a source file
- Any comment that starts `/**` and ends `*/` is considered a documentation comment
#### Other comments
**Style note:** "When declaring multiple variables of the same type with a single statement, it is a common practice to write each variable name on a separate line with a comment explaining the variable's purpose":
```java
int fahrenheit, //Temperature in F
	celsius, //Temperature in C
	kelvin; //Temperature in K
```
### Reading keyboard input
- `System.in` object = standard input device, probably the keyboard
- `System.in` reads input only in byte values
- Solution: wrap it in the `Scanner` class 

```java
Scanner keyboard = new Scanner(System.in);
```
- `Scanner keyboard` declares a variable that's a `Scanner` object named `keyboard`
- `new` creates an object in memory (for reference variables)
- `= new Scanner(System.in);` = Scanner object that will read from the stdin
- "The `Scanner` class has methods for reading strings, `bytes`, integers, `long` integers, `short` integers, `floats`, and `doubles`."
- Must **import `java.util.Scanner`**

- Conflict between different Scanner methods: the type-specific ones skip newlines, `nextLine` does not
- Example of how this is a problem: "`nextDouble` method is designed to skip any leading newline characters it encounters ... Because the `nextDouble` method, back in line 24, left a newline character in the keyboard buffer, the `nextLine` method will not read any input."
- Because `newline` characters are created by the user pressing enter on stdin
- Flush the buffer by consuming the extra newline with `keyboard.nextLine();` assigned to nothing

> There are probably more elegant ways to handle this? -> [[Java-Scanner-hax]]

### Dialog boxes with JavaX
- Major types of dialog box: *message dialog* (displays a message and an OK button) and *input dialog* (prompt and a text field, then OK and Cancel)
- `import javax.swing.JOptionPane` -> `JOptionPane` class for making easy dialog boxes 
- Mixing `JOptionPane` and console programs is messy: requires `System.exit(0);` to close out the thread using `JOptionPane`
- `System.exit(0)` is usually a bad idea: "unconditionally shuts down the JVM, bypassing the program's normal, logical flow"

For nicer cleanup, [[JOptionPane-Java-damage-control]] (`Window` object cleanup)

- `JOptionPane.showInputDialog(...)` always returns `String` for the user's input