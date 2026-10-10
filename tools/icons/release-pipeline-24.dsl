<!-- Release Pipeline icon - 24x24
     介于 16 与 32 之间的过渡尺寸，单独设计：
       - 两根柱子（阶段 2、3），与 32px 一致：丢弃阶段 1
       - 描边 2.5px，取 16（2）与 32（3.2）之间；
         仍小于等比缩放值 2.4 的可用余量，故略加粗保证清晰
       - 箭杆加长，尾端搭在柱 2 顶部，形成连贯的对角线
     构造法与主图一致。尖端 T=(19.5, 6.5)，描边 S=2.5，翼长 Lw=7.5，杆长 Ls=10。
     箭杆定位（绕中心顺时针 45°，上端中心落在 T）：
         left = T.x - 0.3536*Ls - S/2
         top  = T.y + 0.3536*Ls - Ls/2
     竖翼顶部比尖端上探 0.63px（=0.25*S）闭合接缝。 -->
<Snapshot type="png" background="transparent">
    <Container
        width="24" height="24"
        borderRadius="5.5"
        gradientType="LINEAR"
        gradientColors="#0B1B4D,#1E3A8A,#5B21B6"
        gradientStops="0,0.55,1"
        gradientBegin="TOP_LEFT"
        gradientEnd="BOTTOM_RIGHT">

        <Stack>

            <!-- 阶段 2：进行中 -->
            <Positioned left="4" bottom="3.5" width="3.5" height="5">
                <Container color="#22D3EEE6" borderRadius="1.3"/>
            </Positioned>

            <!-- 阶段 3：发布中 -->
            <Positioned left="9" bottom="3.5" width="3.5" height="8">
                <Container color="#67E8F9" borderRadius="1.3"/>
            </Positioned>

            <!-- 箭头横翼：从尖端向左 -->
            <Positioned left="12" top="5.25" width="7.5" height="2.5">
                <Container color="#F0F9FF" borderRadius="1.25"/>
            </Positioned>

            <!-- 箭头竖翼：从尖端向下，顶部上探 0.63px 闭合接缝 -->
            <Positioned left="18.25" top="5.87" width="2.5" height="8.13">
                <Container color="#F0F9FF" borderRadius="1.25"/>
            </Positioned>

            <!-- 箭杆：旋转 45°，上端落在尖端，尾端搭在柱 2 顶部 -->
            <Positioned left="14.71" top="5.04" width="2.5" height="10">
                <Transform
                    matrix="(0.7071,0.7071,0,0,-0.7071,0.7071,0,0,0,0,1,0,0,0,0,1)"
                    alignment="CENTER">
                    <Container width="2.5" height="10" color="#F0F9FF" borderRadius="1.25"/>
                </Transform>
            </Positioned>

        </Stack>
    </Container>
</Snapshot>