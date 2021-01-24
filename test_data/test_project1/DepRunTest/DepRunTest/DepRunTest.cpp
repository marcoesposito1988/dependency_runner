// DepRunTest.cpp : Defines the entry point for the application.
//

#include<DepRunTestLib.h>

using namespace std;

int main()
{
	testFunction();
	TestClass tc;
	tc.testMethod(3);
	TestClass::testStaticMethod(4);
	return 0;
}
