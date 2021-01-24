
#include <depruntestlib_export.h>


DEPRUNTESTLIB_EXPORT void testFunction();


class DEPRUNTESTLIB_EXPORT TestClass {
public:
	TestClass();

	float testMethod(int x);

	static int testStaticMethod(int z);
private:
	int y;
};
